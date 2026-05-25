// Bridge between the LLM (candle) and the note buffer.
//
// Platform selection is isolated in ModelConfig::for_platform().
// Adding a new platform = one new cfg branch in that function.

use crate::note_buffer::NoteBuffer;
use crate::presets::Preset;
use crate::NoteEvent;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::qwen2::{Config as Qwen2Config, ModelForCausalLM};
use hf_hub::{api::sync::ApiBuilder, Repo, RepoType};
use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokenizers::Tokenizer;

// ─────────────────────────────────────────────────────────────
//  MODEL CONFIGURATION (platform-dependent)
// ─────────────────────────────────────────────────────────────

pub struct ModelConfig {
    pub hf_repo:     &'static str,
    pub description: &'static str,
    // Some(_) = bundled local path (mobile); None = download via hf-hub (desktop)
    pub local_path:  Option<PathBuf>,
}

impl ModelConfig {
    pub fn for_platform() -> Self {
        #[cfg(target_os = "android")]
        {
            Self {
                hf_repo:     "HuggingFaceTB/SmolLM2-135M-Instruct",
                description: "SmolLM2-135M (bundled, Android)",
                local_path:  Some(PathBuf::from(
                    "/data/data/com.continuum.app/files/smollm2-135m",
                )),
            }
        }

        // To add iOS: copy the Android branch, adjust local_path.

        #[cfg(not(target_os = "android"))]
        {
            Self {
                hf_repo:     "Qwen/Qwen2.5-0.5B-Instruct",
                description: "Qwen2.5-0.5B (downloaded, desktop)",
                local_path:  None,
            }
        }
    }

    pub fn resolve_path(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(ref path) = self.local_path {
            if !path.exists() {
                return Err(format!(
                    "bundled model not found at {:?} — \
                     make sure the model files are in the app assets",
                    path
                )
                .into());
            }
            return Ok(path.clone());
        }

        println!("[llm] checking model cache for {}...", self.hf_repo);
        let api  = ApiBuilder::new()
            .with_endpoint("https://huggingface.co".to_string())
            .build()?;
        let repo = api.repo(Repo::new(self.hf_repo.to_string(), RepoType::Model));

        let required = [
            "tokenizer.json",
            "tokenizer_config.json",
            "config.json",
            "model.safetensors",
        ];
        let optional = ["generation_config.json", "special_tokens_map.json"];

        let mut model_dir: Option<PathBuf> = None;

        for file in &required {
            print!("[llm] fetching {} ... ", file);
            let path = repo.get(file)?;
            println!("ok");
            if model_dir.is_none() {
                model_dir = path.parent().map(|p| p.to_path_buf());
            }
        }
        for file in &optional {
            print!("[llm] fetching {} ... ", file);
            match repo.get(file) {
                Ok(_)  => println!("ok"),
                Err(e) => println!("skipped ({})", e),
            }
        }

        let dir = model_dir.ok_or("could not determine model directory")?;
        println!("[llm] model ready: {:?}", dir);
        Ok(dir)
    }
}

// ─────────────────────────────────────────────────────────────
//  JSON RESPONSE PARSING
// ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct LlmNote {
    note:     u8,
    velocity: f32,
    duration: f32,
}

#[derive(Deserialize, Debug)]
struct LlmResponse {
    notes: Vec<LlmNote>,
}

// Extract the JSON object from raw model output and convert to NoteEvents.
// The model sometimes emits preamble or trailing text; we isolate the JSON
// block by finding the outermost { ... } pair.
fn parse_notes(raw: &str) -> Vec<NoteEvent> {
    raw.split_whitespace()
        .filter_map(|event_str| {
            let parts: Vec<&str> = event_str.split('|').collect();
            if parts.len() == 3 {
                let note = parts[0].parse::<u8>().ok()?;
                let velocity = parts[1].parse::<f32>().ok()?;
                let duration = parts[2].parse::<f32>().ok()?;
                
                Some(NoteEvent { note, velocity, duration })
            } else {
                None
            }
        })
        .collect()
}

// Convert LlmResponse notes to NoteEvents with validation.
fn convert_notes(resp: LlmResponse) -> Vec<NoteEvent> {
    resp.notes
        .into_iter()
        .filter(|n| (36..=96).contains(&n.note))
        .filter(|n| n.velocity > 0.0 && n.velocity <= 1.0)
        .filter(|n| n.duration >= 100.0 && n.duration <= 4000.0)
        .map(|n| NoteEvent {
            note:     n.note,
            velocity: n.velocity.clamp(0.05, 0.95),
            duration: n.duration,
        })
        .collect()
}

// Attempt to fix a truncated JSON string so serde_json can parse it.
// Strategy: find all complete note objects and wrap them in a valid response.
fn recover_truncated_json(raw: &str) -> String {
    // Find the start of the notes array
    let start = match raw.find('{') {
        Some(i) => i,
        None    => return String::from(r#"{"notes":[]}"#),
    };
    let fragment = &raw[start..];

    // Collect every complete note object: {...} pairs
    let mut notes = Vec::new();
    let mut depth = 0i32;
    let mut obj_start = None;

    for (i, ch) in fragment.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                if depth == 2 { // depth 1 = outer object, depth 2 = note object
                    obj_start = Some(i);
                }
            }
            '}' => {
                if depth == 2 {
                    if let Some(s) = obj_start {
                        let candidate = &fragment[s..=i];
                        // Only keep objects that have the required fields
                        if candidate.contains("\"note\"")
                            && candidate.contains("\"velocity\"")
                            && candidate.contains("\"duration\"")
                        {
                            notes.push(candidate.to_string());
                        }
                    }
                    obj_start = None;
                }
                depth -= 1;
                if depth < 0 { break; }
            }
            _ => {}
        }
    }

    if notes.is_empty() {
        return String::from(r#"{"notes":[]}"#);
    }

    format!("{{\"notes\":[{}]}}", notes.join(","))
}

// ─────────────────────────────────────────────────────────────
//  TOKEN SAMPLING
// ─────────────────────────────────────────────────────────────

// Apply temperature scaling to a logits vector and sample one token.
// temperature: controls randomness (0.1 = deterministic, 1.5 = creative).
// top_p: nucleus sampling — only considers tokens covering top_p of probability mass.
fn sample_token(
    logits:      &[f32],
    temperature: f32,
    top_p:       f32,
    rng:         &mut impl Rng,
) -> u32 {
    // Apply temperature — divide logits before softmax
    let scaled: Vec<f32> = logits.iter().map(|&l| l / temperature).collect();

    // Softmax
    let max = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = scaled.iter().map(|&x| (x - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    let mut probs: Vec<f32> = exps.iter().map(|&e| e / sum).collect();

    // Top-p (nucleus) filtering
    // Sort indices by probability descending, keep tokens until cumulative prob >= top_p
    let mut indexed: Vec<(usize, f32)> = probs.iter().copied().enumerate().collect();
    indexed.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut cumulative = 0.0f32;
    let mut keep = 0usize;
    for (i, (_, p)) in indexed.iter().enumerate() {
        cumulative += p;
        keep = i + 1;
        if cumulative >= top_p {
            break;
        }
    }
    // Zero out tokens outside the nucleus
    let kept_indices: std::collections::HashSet<usize> =
        indexed[..keep].iter().map(|&(idx, _)| idx).collect();
    for (i, p) in probs.iter_mut().enumerate() {
        if !kept_indices.contains(&i) {
            *p = 0.0;
        }
    }
    // Re-normalize
    let new_sum: f32 = probs.iter().sum();
    probs.iter_mut().for_each(|p| *p /= new_sum);

    // Weighted random sample
    let dist = WeightedIndex::new(&probs).unwrap();
    dist.sample(rng) as u32
}

// ─────────────────────────────────────────────────────────────
//  MODEL LOADING
// ─────────────────────────────────────────────────────────────

struct LoadedModel {
    model:     ModelForCausalLM,
    tokenizer: Tokenizer,
    eos_token: u32,
    device:    Device,
}

fn load_model(model_dir: &PathBuf) -> Result<LoadedModel, Box<dyn std::error::Error>> {
    let device = Device::Cpu;

    // Load tokenizer
    let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
        .map_err(|e| format!("tokenizer error: {}", e))?;
    println!("[llm] tokenizer loaded");

    // Find the EOS token id — used to stop generation
    // Qwen2 uses "<|im_end|>" as EOS in chat format
    let eos_token = tokenizer
        .token_to_id("<|im_end|>")
        .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
        .unwrap_or(151645); // Qwen2 fallback EOS id
    println!("[llm] EOS token id: {}", eos_token);

    // Load model config
    let config_str = std::fs::read_to_string(model_dir.join("config.json"))?;
    let config: Qwen2Config = serde_json::from_str(&config_str)
        .map_err(|e| format!("config parse error: {}", e))?;
    println!("[llm] model config loaded (hidden_size: {})", config.hidden_size);

    // Load weights from safetensors using memory-mapped file (fast, low RAM overhead)
    println!("[llm] loading weights (this may take a moment)...");
    let weights_path = model_dir.join("model.safetensors");
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DType::F32, &device)?
    };

    let model = ModelForCausalLM::new(&config, vb)?;
    println!("[llm] model loaded successfully");

    Ok(LoadedModel { model, tokenizer, eos_token, device })
}

// ─────────────────────────────────────────────────────────────
//  AUTOREGRESSIVE GENERATION
// ─────────────────────────────────────────────────────────────

// Generate up to max_new_tokens tokens autoregressively.
// Returns the decoded string (new tokens only, not the prompt).
//
// The KV-cache inside ModelForCausalLM accumulates across calls within
// one generation pass — we reset it before each new prompt.
fn generate(
    loaded:         &mut LoadedModel,
    prompt:         &str,
    max_new_tokens: usize,
    temperature:    f32,
    top_p:          f32,
) -> Result<String, Box<dyn std::error::Error>> {
    // Reset KV-cache so previous generation does not bleed into this one
    loaded.model.clear_kv_cache();

    // Tokenize the prompt
    let encoding = loaded.tokenizer
        .encode(prompt, true)
        .map_err(|e| format!("tokenize error: {}", e))?;
    let prompt_ids: Vec<u32> = encoding.get_ids().to_vec();
    let prompt_len = prompt_ids.len();

    let mut all_ids = prompt_ids.clone();
    let mut rng = rand::rng();

    // Feed the full prompt in one forward pass (prefill)
    let input = Tensor::new(prompt_ids.as_slice(), &loaded.device)?
        .unsqueeze(0)?; // shape: [1, prompt_len]
    let logits = loaded.model.forward(&input, 0)?; // seqlen_offset = 0 for prefill

    // Get logits for the last token position: shape [vocab_size]
    let last_logits = logits
        .squeeze(0)?            // [seq, vocab]
        .get(logits.dim(1)? - 1)?; // last position

    let logits_vec: Vec<f32> = last_logits.to_vec1()?;
    let next_token = sample_token(&logits_vec, temperature, top_p, &mut rng);
    all_ids.push(next_token);

    // Autoregressive decode loop — one token at a time
    for step in 1..max_new_tokens {
        if next_token == loaded.eos_token {
            break;
        }

        // Feed only the last generated token; KV-cache handles the history
        let input = Tensor::new(&[*all_ids.last().unwrap()], &loaded.device)?
            .unsqueeze(0)?; // shape: [1, 1]

        // seqlen_offset = how many tokens are already in the KV-cache
        let seqlen_offset = prompt_len + step - 1;
        let logits = loaded.model.forward(&input, seqlen_offset)?;

        let last_logits = logits.squeeze(0)?.get(0)?;
        let logits_vec: Vec<f32> = last_logits.to_vec1()?;
        let next = sample_token(&logits_vec, temperature, top_p, &mut rng);
        all_ids.push(next);

        // Stop at EOS or if we have found a complete JSON object
        if next == loaded.eos_token {
            break;
        }
    }

    // Decode only the newly generated tokens (skip the prompt)
    let new_ids = &all_ids[prompt_len..];
    let text = loaded.tokenizer
        .decode(new_ids, true)
        .map_err(|e| format!("decode error: {}", e))?;

    // The prompt already ends with {"notes":[ so we prepend it back
    // so parse_notes sees a complete JSON object.
    Ok(text)
}

// ─────────────────────────────────────────────────────────────
//  MAIN GENERATION LOOP
// ─────────────────────────────────────────────────────────────

pub fn run_llm(
    buffer: Arc<NoteBuffer>,
    preset: &Preset,
) -> Result<(), Box<dyn std::error::Error>> {
    let model_config = ModelConfig::for_platform();
    println!("[llm] using model: {}", model_config.description);

    let model_dir = model_config.resolve_path()?;
    let mut loaded = load_model(&model_dir)?;

    let system_prompt = preset.system_prompt();
    println!("[llm] generation loop started | preset: {}", preset.name);

    let mut batch = 0u32;

    loop {
        buffer.wait_until_refill_needed();
        batch += 1;
        println!("[llm] batch #{} | buffer: {} notes", batch, buffer.len());

        // Qwen2 chat template format.
        // We prime the assistant turn with the opening brace so the model
        // starts generating JSON immediately without any preamble.
        let prompt = format!(
            "<|im_start|>system\n{}<|im_end|>\n\
             <|im_start|>user\nGenerate the next musical phrase.<|im_end|>\n\
             <|im_start|>assistant\n",
            system_prompt
        );

        // Generate up to 512 tokens; a full JSON phrase for 15-20s of music
        // typically needs 150-300 tokens at this note density.
        let raw = match generate(&mut loaded, &prompt, 512, 0.95, 0.95) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("[llm] generation error: {} — retrying in 1s", e);
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        println!("[llm] raw output: {}", &raw[..raw.len().min(200)]);

        let notes = parse_notes(&raw);

        if notes.is_empty() {
            eprintln!("[llm] no valid notes parsed — retrying");
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }

        // Log how many seconds of music this batch covers
        let total_ms: f32 = notes.iter().map(|n| n.duration).sum();
        println!(
            "[llm] parsed {} notes ({:.1}s of music) — pushing to buffer",
            notes.len(),
            total_ms / 1000.0
        );

        buffer.push_batch(notes);
    }
}
