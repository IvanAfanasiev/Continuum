use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Proto-гибрид: аранжировка pure-formula прототипа (фиксированные
/// маски, явная 8-тактовая дисциплина, один лид), звук — голоса
/// основного движка (фильтры, огибающие, стерео, мастер-цепь).
/// Ядро худое (5 слотов): кик, саб, снейр, хэты, один щипок.
/// Для живого A/B с club в приложении.
const SCALE: [u8; 7] = [33, 35, 36, 38, 40, 41, 43];

/// Кик — абсолютная четверка, как в прототипе (там бит и держится).
const KICK_PH: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
    0.0,
];
/// Снейр — half-time прототипа: только 3-я доля, без гостов.
const SNARE_PH: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0,
];
/// Хэты — оффбитные 8-е сильные, 16-е шепотом (маска прототипа).
const HAT_PH: &[f32] = &[
    0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35,
    1.0, 0.35,
];
/// Саб — корни на 1 и 3 долгими нотами (в прототипе бас — дрон
/// по корням прогрессии). Квинт нет: чистый фундамент.
const BASS_PH: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0,
];
/// Лид — маска прототипа один в один: якоря на четвертях, жизнь между.
const LEAD_PH: &[f32] = &[
    1.0, 0.0, 0.4, 0.0, 0.6, 0.0, 0.4, 0.0, 0.7, 0.0, 0.4, 0.0, 0.5, 0.0, 0.4,
    0.2,
];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0,
];

pub struct ProtoclubStyle {}

impl ProtoclubStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ProtoclubStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for ProtoclubStyle {
    fn name(&self) -> &str {
        "proto"
    }

    fn bpm(&self) -> f32 {
        126.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Почти прямо, как в прототипе: кач — слайдами и грувом.
        1.1
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn reverb_wet(&self) -> f32 {
        0.1
    }

    fn reverb_size(&self) -> f32 {
        0.4
    }

    fn treble_cutoff(&self) -> f32 {
        15000.0
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Худое ядро прототипа: кик, саб, снейр, хэты, один щипок.
        &[Slot::Bass, Slot::Kick, Slot::Snare, Slot::Hat, Slot::Pluck]
    }

    /// Гость — хор по сиду (есть/нет): худоба ядра, разнообразие сидов.
    fn flex_pool(&self) -> &'static [Slot] {
        &[Slot::Vocal]
    }

    fn flex_weights(&self) -> &'static [f32] {
        &[1.0]
    }

    fn flex_range(&self) -> (usize, usize) {
        (0, 1)
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Саб держит корни (ступень 0, без квинт и прыжков),
        // регистр — фолдом, как везде.
        let bass = RoleGuide {
            energy_response: 1.5,
            duration_mult: 3.0,
            vel_base: 0.85,
            vel_energy: 0.15,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 0,
            },
            octave_up: 0.0,
            top_pitch: 48,
            vel_high_cut: 0.6,
            bass_fifth: false,
            sub_bias: 0.8,
            bass_top: 43,
            ..RoleGuide::pitched(BASS_PH, 0, 0, 0, 3.0, 0.85)
        };
        let kick = RoleGuide {
            vel_base: 1.0,
            vel_energy: 0.0,
            vel_jitter: 0.02,
            ..RoleGuide::kit(KICK_PH, 36, 1.0)
        };
        let pad = RoleGuide {
            duration_mult: 8.0,
            vel_base: 0.25,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 8.0, 0.25)
        };
        let snare = RoleGuide {
            vel_base: 0.9,
            vel_jitter: 0.04,
            ..RoleGuide::kit(SNARE_PH, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            duration_mult: 0.4,
            vel_base: 0.75,
            vel_energy: 0.25,
            vel_jitter: 0.08,
            ..RoleGuide::kit(HAT_PH, 42, 0.75)
        };
        // Единственный лид: мотив сида, октава из общей ротации темы.
        // Второй темы нет — солист один, как в прототипе.
        let pluck = RoleGuide {
            energy_response: 1.8,
            duration_mult: 0.9,
            vel_base: 0.55,
            vel_energy: 0.3,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            follow_motif: true,
            ..RoleGuide::pitched(LEAD_PH, 0, 2, 1, 0.9, 0.55)
        };
        let keys = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.0, 0.3)
        };
        let kick808 = RoleGuide {
            vel_base: 0.5,
            ..RoleGuide::kit(BED_W, 36, 0.5)
        };
        let laser = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 2, 1.0, 0.3)
        };
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.2)
        };
        let vocal = RoleGuide {
            energy_response: 1.2,
            duration_mult: 3.0,
            vel_base: 0.45,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 2, 2, 2, 3.0, 0.45)
        };
        let flute = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.5, 0.3)
        };
        let festival = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 2, 1.0, 0.3)
        };
        let guitar = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.5, 0.3)
        };
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.2)
        };
        let fuzz = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.0, 0.3)
        };
        let shaker = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.4,
            vel_base: 0.3,
            vel_jitter: 0.06,
            ..RoleGuide::kit(BED_W, 60, 0.3)
        };
        let formula = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 1, 1.0, 0.3)
        };
        // Вейвтейбл вне автомикса: ручная краска для фронта.
        let wavetable = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.0,
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 2, 1.0, 0.3)
        };
        [
            bass, kick, pad, snare, hat, pluck, keys, kick808, laser, vinyl, vocal, flute,
            festival, guitar, rain, fuzz, shaker, formula, wavetable,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{render_style, StepContext};
    use super::*;
    use crate::playback::PLUCK_ID;

    fn render(seed: u64, steps: usize) -> Vec<(usize, crate::instruments::NoteEvent)> {
        let style = ProtoclubStyle::new();
        let active: Vec<usize> = crate::instruments::Slot::ALL
            .iter()
            .map(|s| s.id())
            .collect();
        let mut rng = rand::SeedableRng::seed_from_u64(seed);
        let mut arranger = crate::arranger::Arranger::new(seed);
        let mut song = arranger.advance_measure();
        let mut out = Vec::new();
        for step in 0..steps {
            let spm = style.steps_per_measure();
            let sim = step % spm;
            if sim == 0 && step > 0 {
                song = arranger.advance_measure();
            }
            let mut sctx = StepContext {
                step,
                step_in_measure: sim,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song,
                rng: &mut rng,
                intensity: 1.0,
            };
            out.extend(
                render_style(&style, &mut sctx)
                    .into_iter()
                    .map(|n| (step, n)),
            );
        }
        out
    }

    #[test]
    fn same_seed_same_notes() {
        let a = render(1234, 32);
        let b = render(1234, 32);
        assert_eq!(a.len(), b.len());
        for ((_, x), (_, y)) in a.iter().zip(b.iter()) {
            assert_eq!(x.instrument_id, y.instrument_id);
            assert_eq!(x.midi_pitch, y.midi_pitch);
        }
    }

    #[test]
    fn snare_is_half_time() {
        // Маска прототипа: снейр только на 3-й доле, бэкбита на 2 нет.
        assert_eq!(SNARE_PH[8], 1.0, "snare must crack on 3");
        assert_eq!(SNARE_PH[4], 0.0, "no backbeat on 2");
    }

    #[test]
    fn single_lead_carries_theme() {
        // Солист один: только плак ведет тему, второго носителя нет.
        let style = ProtoclubStyle::new();
        let guides = style.slot_guides();
        let carriers: Vec<_> = crate::instruments::Slot::ALL
            .iter()
            .enumerate()
            .filter(|(i, _)| guides[*i].follow_motif && !guides[*i].is_silent())
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(carriers, vec![crate::instruments::Slot::Pluck]);
    }

    #[test]
    fn pluck_sounds_in_proto_mix() {
        let notes = render(7, 256);
        let hits = notes
            .iter()
            .filter(|(_, n)| n.instrument_id == PLUCK_ID)
            .count();
        assert!(hits >= 8, "lead silent in proto: {hits}");
    }
}
