// Bridge between the LLM (candle) and the note buffer.

// Platform selection is isolated in ModelConfig::for_platform() so
// the rest of the code is unaware of the Android vs desktop difference.
// Adding a new platform means adding one cfg branch in that function.

use crate::note_buffer::NoteBuffer;
use crate::presets::Preset;
use crate::NoteEvent;
use candle_core::Device;
use hf_hub::{api::sync::ApiBuilder, Repo, RepoType};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokenizers::Tokenizer;

// ─────────────────────────────────────────────────────────────
//  MODEL CONFIGURATION (platform-dependent)
// ─────────────────────────────────────────────────────────────

pub struct ModelConfig {
    pub hf_repo: &'static str,
    pub description: &'static str,
    // Some(_) = bundled local path (mobile); None = download via hf-hub (desktop).
    pub local_path: Option<PathBuf>,
}

impl ModelConfig {
    pub fn for_platform() -> Self {
        #[cfg(target_os = "android")]
        {
            // The model is bundled with the APK and copied to the app's files dir
            // by the Android NDK asset pipeline.
            Self {
                hf_repo: "HuggingFaceTB/SmolLM2-135M-Instruct",
                description: "SmolLM2-135M (bundled, Android)",
                local_path: Some(PathBuf::from(
                    "/data/data/com.continuum.app/files/smollm2-135m",
                )),
            }
        }

        #[cfg(not(target_os = "android"))]
        {
            // Desktop (Windows / Linux / macOS): download on first run,
            // then served from the hf-hub cache (~/.cache/huggingface/hub/).
            Self {
                hf_repo: "Qwen/Qwen2.5-0.5B-Instruct",
                description: "Qwen2.5-0.5B (downloaded, desktop)",
                local_path: None,
            }
        }
    }

    // Resolve the directory containing all model files.
    // On mobile, validates the bundled path. On desktop, downloads any missing
    // files via hf-hub (subsequent runs hit the local cache instantly).
    pub fn resolve_path(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(ref path) = self.local_path {
            if !path.exists() {
                return Err(format!(
                    "bundled model not found at {:?} — \
                     make sure the model files are included in the app assets",
                    path
                )
                .into());
            }
            return Ok(path.clone());
        }

        println!("[llm] checking model cache for {}...", self.hf_repo);
        let api = ApiBuilder::new()
            .with_endpoint("https://huggingface.co".to_string())
            .build()?;
        let repo = api.repo(Repo::new(self.hf_repo.to_string(), RepoType::Model));

        // hf-hub 0.3 has no snapshot_download, so we request each file
        // individually. Files already in the cache are returned immediately.
        let required = ["tokenizer.json", "tokenizer_config.json", "config.json", "model.safetensors"];
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
    note: u8,
    velocity: f32,
    duration: f32,
}

#[derive(Deserialize, Debug)]
struct LlmResponse {
    notes: Vec<LlmNote>,
}

// Extract a valid JSON object from the raw model output.
// The model sometimes emits preamble or trailing text; we find the first
// '{' and last '}' to isolate the JSON block before parsing.
fn parse_notes(raw: &str) -> Vec<NoteEvent> {
    let (Some(start), Some(end)) = (raw.find('{'), raw.rfind('}')) else {
        eprintln!("[llm] no JSON found in response: {:?}", &raw[..raw.len().min(120)]);
        return vec![];
    };

    match serde_json::from_str::<LlmResponse>(&raw[start..=end]) {
        Ok(resp) => resp
            .notes
            .into_iter()
            .filter(|n| (36..=96).contains(&n.note))
            .filter(|n| n.velocity > 0.0 && n.velocity <= 1.0)
            .filter(|n| n.duration >= 50.0 && n.duration <= 5000.0)
            .map(|n| NoteEvent {
                note: n.note,
                velocity: n.velocity.clamp(0.05, 0.95),
                duration: n.duration,
            })
            .collect(),
        Err(e) => {
            eprintln!("[llm] JSON parse error: {}", e);
            vec![]
        }
    }
}

// ─────────────────────────────────────────────────────────────
//  MAIN GENERATION LOOP
// ─────────────────────────────────────────────────────────────

// Loads the model, then loops forever: sleeps while the buffer is full,
// generates a batch of notes when it drops below the refill threshold,
// and pushes the parsed notes back into the buffer.
pub fn run_llm(
    buffer: Arc<NoteBuffer>,
    preset: &Preset,
) -> Result<(), Box<dyn std::error::Error>> {
    let model_config = ModelConfig::for_platform();
    println!("[llm] using model: {}", model_config.description);

    let model_dir = model_config.resolve_path()?;
    let _device   = Device::Cpu; // CPU for maximum cross-platform compatibility

    let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
        .map_err(|e| format!("tokenizer load error: {}", e))?;
    println!("[llm] tokenizer loaded");

    let system_prompt = preset.system_prompt();
    println!("[llm] generation loop started | preset: {}", preset.name);

    let mut batch_number = 0u32;

    loop {
        // Sleep until the composer has consumed enough notes to need a refill.
        buffer.wait_until_refill_needed();

        batch_number += 1;
        println!(
            "[llm] generating batch #{} (buffer: {} notes)",
            batch_number,
            buffer.len()
        );

        let prompt = format!(
            "<|system|>\n{}\n<|user|>\nGenerate the next musical phrase.\n<|assistant|>\n",
            system_prompt
        );

        let encoding = tokenizer
            .encode(prompt.as_str(), true)
            .map_err(|e| format!("tokenize error: {}", e))?;

        let _input_ids: Vec<u32> = encoding.get_ids().to_vec();

        // TODO: replace this stub with a real candle forward pass.
        
        let raw_output = stub_generate();

        let notes = parse_notes(&raw_output);

        if notes.is_empty() {
            eprintln!("[llm] empty parse result — retrying in 500ms");
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }

        println!("[llm] parsed {} notes — pushing to buffer", notes.len());
        buffer.push_batch(notes);
    }
}

// ─────────────────────────────────────────────────────────────
//  STUB GENERATOR
//
//  Returns a hard-coded JSON phrase so the full pipeline
//  (LLM -> buffer -> composer -> audio) can be exercised before
//  the real candle forward pass is wired in.
//
//  Replace the body of run_llm's generation step with a real
//  autoregressive loop once you integrate the candle model.
// ─────────────────────────────────────────────────────────────

fn stub_generate() -> String {
    eprintln!("[llm] WARNING: stub generator active — replace with real forward pass");
    r#"{"notes":[
        {"note":60,"velocity":0.6,"duration":400},
        {"note":64,"velocity":0.5,"duration":400},
        {"note":67,"velocity":0.7,"duration":800},
        {"note":65,"velocity":0.5,"duration":400},
        {"note":69,"velocity":0.6,"duration":400},
        {"note":72,"velocity":0.4,"duration":1200}
    ]}"#
    .to_string()
}
