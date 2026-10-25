use super::riff::RiffKind;
use super::transition::{Kind, Spec};
use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Phonk: мемфис-наследие без колокола. Жмущий кик, слайдящий 808,
/// тёмные клавиши-стабы и хор-призрак. Прямо, темно, медленно качает.
/// Темп и краски предварительные — точная калибровка по референсу
/// пользователя (пример трека) следующим шагом.
const SCALE: [u8; 7] = [45, 47, 48, 50, 52, 53, 55];

const KICK_PH: &[f32] = &[
    // Реф: напористый нок почти весь такт (XXXXXxXXX.X.XxXx).
    // Слабые доли — тише И реже (0.4–0.55): ровные 0.9 давали
    // пулеметную стену вместо грува. Якоря держат четверти.
    // Вдох перед единицей свят: 15-й шаг пуст во всех стилях.
    1.0, 0.55, 0.55, 0.55, 1.0, 0.4, 0.7, 0.55, 1.0, 0.0, 0.9, 0.0, 1.0, 0.0, 0.4, 0.0,
];
const SNARE_PH: &[f32] = &[
    // Реф: half-time — снейр только на 3 (шаг 8), не на 2 и 4.
    // Госты — едва слышный шепот, иначе снейр скачет по такту.
    0.0, 0.0, 0.1, 0.0, 0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.15, 0.0, 0.0, 0.1, 0.0,
];
const HAT_PH: &[f32] = &[
    0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.4, 0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.5,
];
const BASS_PH: &[f32] = &[
    0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KEYS_PH: &[f32] = &[
    0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0,
];
const SUB_PH: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.6, 0.0,
];
const VOCAL_PH: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Песок поверх хэтов: плотные 16-е рефа гудят верхом,
// колокола нет — воздух дают шейкер и хэт.
const SHK_PH: &[f32] = &[
    0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.4,
];

pub struct PhonkStyle {}

impl PhonkStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for PhonkStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for PhonkStyle {
    fn name(&self) -> &str {
        "phonk"
    }

    fn bpm(&self) -> f32 {
        140.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Почти прямо с лёгкой походкой (реф: шатание нечётных ~1.1):
        // кач — 808-слайдами, а не свингом.
        1.1
    }

    fn style_contract(&self) -> super::contract::StyleContract {
        // Phonk: бас уступает кику, не больше двух мелодических онсетов
        // на шаг. Верхний регистр без ограничения (стиль кладёт
        // вокал/808 выше и это лицо стиля).
        super::contract::StyleContract::new(true, true, 2, None)
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    /// Форма стиля: свои длины секций — стиль должен отличаться
    /// не тембрами, а построением.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::DRIFT_SCORE
    }

    /// Аранж фонка: дрейф — минимум голосов, тема почти голая (808 и
    /// бочка). Клавиши приходят в разгоне, вокал — в дропе.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{default_layers, L_GROOVE, L_HARMONY};
        let base = default_layers();
        [
            // Тема: только грув (808 + бочка) — дрейф идет в пустоте.
            L_GROOVE,
            // Разгон: подключаются клавиши.
            L_GROOVE | L_HARMONY,
            // Удар: весь состав.
            base[2],
            // Передышка: half-time, голоса молчат.
            L_GROOVE,
        ]
    }

    /// Phonk входит в удар слайдом 808: квинта и корень в последнем
    /// такте разгона, короткий хвост на даунбите дропа.
    fn transition_gesture(&self) -> Option<Spec> {
        Some(Spec::new(
            Kind::Slide808,
            &[Slot::Kick808],
            &[Slot::Kick808],
            2,
            36,
        ))
    }

    fn riff_kind(&self) -> RiffKind {
        RiffKind::Phonk
    }

    fn reverb_wet(&self) -> f32 {
        0.12
    }

    fn reverb_size(&self) -> f32 {
        0.4
    }

    /// Верх по референсу (centroid ~3.3k, HF ~0.22 — как клуб):
    /// темно только внизу, колокол выведен, а не зарезан фильтром.
    fn treble_cutoff(&self) -> f32 {
        15000.0
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Ядро без колокола: кик, 808, саб, снейр, хэты, шейкер-песок
        // и тёмные клавиши.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Keys,
            Slot::Kick808,
            Slot::Shaker,
        ]
    }

    // Гостей нет: ядро худое и фиксированное, сиды различаются
    // грувом и темой. Вокальная группа выведена из набора.

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.8,
            gates: &[1.0, 1.5, 2.0],
            tie_repeats: 0.5,
            vel_base: 0.8,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            octave_up: 0.05,
            top_pitch: 48,
            vel_high_cut: 0.6,
            bass_fifth: true,
            sub_bias: 0.6,
            // Реф: 808 сидит на F2 (86 Гц) — кап 41, ниже клубного.
            bass_top: 41,
            ..RoleGuide::pitched(BASS_PH, 0, -1, 0, &[1.0, 1.5, 2.0], 0.8)
        };
        let kick = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            ..RoleGuide::kit(KICK_PH, 36, 0.95)
        };
        let pad = RoleGuide {
            gates: &[6.0, 8.0, 10.0],
            vel_base: 0.25,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[6.0, 8.0, 10.0], 0.25)
        };
        let snare = RoleGuide {
            vel_base: 0.9,
            vel_jitter: 0.04,
            ..RoleGuide::kit(SNARE_PH, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            // Фонк-хэт — закрытый: открытый хэт не был частью рисунка.
            gates: &[0.35, 0.7],
            vel_base: 0.42,
            vel_energy: 0.3,
            vel_jitter: 0.08,
            ..RoleGuide::kit(HAT_PH, 42, 0.7)
        };
        let pluck = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0],
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, &[1.0], 0.3)
        };
        // Тёмные мемфис-стабы: низкая октава, мотив держат.
        let keys = RoleGuide {
            energy_response: 1.5,
            gates: &[0.9, 1.2, 1.6],
            vel_base: 0.55,
            vel_energy: 0.2,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 1,
            },
            follow_motif: true,
            phrase_rest: 0.2,
            ..RoleGuide::pitched(KEYS_PH, 0, 0, 1, &[0.9, 1.2, 1.6], 0.55)
        };
        // 808 держит корни прогрессии на своей сетке (SUB_PH) в
        // суб-регистре рефа; слайды между нотами делает голос Kick808.
        let kick808 = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            bass_top: 41,
            ..RoleGuide::pitched(SUB_PH, 0, -1, 0, &[1.0], 0.95)
        };
        let vinyl = RoleGuide {
            gates: &[200.0],
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[200.0], 0.2)
        };
        // Хор-призрак: редкие долгие в эхе, сидят сзади.
        let vocal = RoleGuide {
            energy_response: 1.2,
            gates: &[2.0, 3.0, 4.0],
            vel_base: 0.45,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 3,
            },
            ..RoleGuide::pitched(VOCAL_PH, 2, 2, 3, &[2.0, 3.0, 4.0], 0.45)
        };
        let guitar = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, &[1.0, 1.5, 2.0], 0.3)
        };
        // Дождь: ровная пелена во все фразы (тише винила и музыки).
        let rain = RoleGuide {
            gates: &[150.0],
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[150.0], 0.35)
        };
        let shaker = RoleGuide {
            energy_response: 1.2,
            gates: &[0.4],
            vel_base: 0.55,
            vel_energy: 0.2,
            vel_jitter: 0.08,
            ..RoleGuide::kit(SHK_PH, 60, 0.55)
        };
        // Соло-гитара вне автомикса phonk: ручная краска для фронта.
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 1, &[1.0, 1.5, 2.0], 0.3)
        };
        [
            bass,
            kick,
            pad,
            snare,
            hat,
            pluck,
            keys,
            kick808,
            vinyl,
            vocal,
            guitar,
            rain,
            shaker,
            guitar_lead,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{render_style, StepContext};
    use super::*;

    fn render(seed: u64, steps: usize) -> Vec<(usize, crate::instruments::NoteEvent)> {
        let style = PhonkStyle::new();
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
    fn kick808_stays_sub() {
        // 808 — бас, а не лид: регистровый кап рефа (F2).
        let style = PhonkStyle::new();
        let guides = style.slot_guides();
        let idx = Slot::ALL.iter().position(|s| *s == Slot::Kick808).unwrap();
        assert_eq!(guides[idx].bass_top, 41, "808 must share the sub ceiling");
    }

    #[test]
    fn snare_is_half_time() {
        // Реф: снейр только на 3 (шаг 8), на 2-м шаге тишина.
        assert_eq!(SNARE_PH[8], 1.0, "snare must crack on 3");
        assert_eq!(SNARE_PH[4], 0.0, "no backbeat on 2: phonk is half-time");
    }
}
