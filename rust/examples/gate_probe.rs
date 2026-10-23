//! Диагностика длин: какие затворы реально выбирает генерация по слоям.
//! Печатает словарь гайда и распределение длин за 8 тактов: видно, что
//! длина — решение (диапазон), а не одна константа на инструмент.

use rust_lib_cont::arranger::Arranger;
use rust_lib_cont::instruments::Slot;
use rust_lib_cont::mixer::MixerState;
use rust_lib_cont::playback::apply_style_mix;
use rust_lib_cont::styles::{make_style, render_style, StepContext};
use std::collections::HashMap;

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "deep".to_string());
    let style = make_style(&name).expect("known style");
    println!("--- {name} ---");
    for (i, guide) in style.slot_guides().iter().enumerate() {
        println!("guide {:<10} gates {:?}", Slot::ALL[i].name(), guide.gates);
    }

    let mut mixer = MixerState::new();
    apply_style_mix(&*style, &mut mixer);
    let mut ids = [0usize; rust_lib_cont::core::MAX_ACTIVE_TRACKS];
    let n = mixer.active_ids(&mut ids);
    let active = ids[..n].to_vec();

    let seed = 424242u64;
    let mut rng = rand::SeedableRng::seed_from_u64(seed);
    let mut arranger = Arranger::new(seed);
    let mut song = arranger.advance_measure();
    let base = style.base_step_secs();
    let spm = style.steps_per_measure();
    let mut hist: HashMap<usize, Vec<f32>> = HashMap::new();

    for bar in 0..8 {
        for step in 0..spm {
            let mut sctx = StepContext {
                step: bar * spm + step,
                step_in_measure: step,
                base_step_secs: base,
                active_tracks: &active,
                song: &song,
                rng: &mut rng,
                intensity: 1.0,
            };
            for note in render_style(&*style, &mut sctx) {
                let gate = note.duration_secs / base;
                // Кровати (150+) не музыкальные длины: не засоряем выборку.
                if gate > 0.0 && gate < 50.0 {
                    hist.entry(note.instrument_id).or_default().push(gate);
                }
            }
        }
        song = arranger.advance_measure();
    }

    for slot in Slot::ALL {
        let Some(v) = hist.get(&slot.id()) else {
            continue;
        };
        let mut v = v.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut classes: Vec<i32> = v.iter().map(|g| g.round() as i32).collect();
        classes.sort_unstable();
        classes.dedup();
        let median = v[v.len() / 2];
        println!(
            "{:<10} n={:4} min={:5.2} med={:5.2} max={:5.2} classes={:?}",
            slot.name(),
            v.len(),
            v[0],
            median,
            v[v.len() - 1],
            classes
        );
    }
}
