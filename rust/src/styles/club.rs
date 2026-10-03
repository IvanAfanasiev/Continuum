use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Club: битмейкер. Прямая бочка + 808-слой, глубокий саб,
/// клэп на 2 и 4, кислота поверх и виниловый фон.
/// Доминирует прямой бит 4/4: тяжело и агрессивно, без свиста.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

const FOUR_FLOOR: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const CLAP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const HAT_W: &[f32] = &[
    0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35,
];
const BASS_W: &[f32] = &[
    // Оффбитные восьмые вместо стены 16-х: кач между ударами бочки,
    // а не ковер под ней. Стена топила транзиенты (crest 4.4 против 7–12
    // у референсов) и темнила микс.
    0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8,
    0.0,
];
const PAD_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const LEAD_W: &[f32] = &[
    1.0, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3,
];
const STAB_W: &[f32] = &[
    0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0,
];
// Клэп с гостами: бэкбит держат якоря, между — шепот шестнадцатых.
const CLAP_GHOST_W: &[f32] = &[
    0.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.4,
];
const SNARE_POLES: &[&[f32]] = &[CLAP_W, CLAP_GHOST_W];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const VOCAL_W: &[f32] = &[
    0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Полюса кика (выбор — сидом на весь трек):
// строгая четверка, синкопа с толчками (ref: Bad Angel),
// драйв восьмыми (ref: Hysteria).
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.6, 0.0,
];
const KICK_DRIVE8_W: &[f32] = &[
    // Вдох перед единицей: 15-й шаг пустой всегда, иначе связка
    // 14-15-0 дает фламовую очередь вместо грува.
    1.0, 0.0, 0.7, 0.0, 1.0, 0.0, 0.7, 0.0, 1.0, 0.0, 0.7, 0.0, 1.0, 0.0, 0.8,
    0.0,
];
// Драйв с оффбитными толчками: четверка плюс синкопа на слабых.
const KICK_PUSH_W: &[f32] = &[
    1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.0, 0.6, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.0,
    0.0,
];
// Шейкер: песок 16-х, оффбитные восьмые — якоря (шейкер ровный),
// четверти и 16-е заполняют.
const SHAKER_W: &[f32] = &[
    0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3,
    1.0, 0.4,
];
const KICK_POLES: &[&[f32]] = &[FOUR_FLOOR, KICK_SYNCOP_W, KICK_DRIVE8_W, KICK_PUSH_W];
const HAT_OFF_W: &[f32] = &[
    0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55,
];
// Плотные шестнадцатые (ref: все три трека держат хэт почти весь такт).
const HAT_BUSY16_W: &[f32] = &[
    0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.4, 0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.5,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_OFF_W, HAT_BUSY16_W];
// Кислота: плотная шестнадцатая линия с акцентами — сквелч течет,
// а не квакает отдельными пинами. Якоря на оффбитах держат кач.
const ACID_W: &[f32] = &[
    0.5, 0.4, 1.0, 0.4, 0.6, 0.4, 1.0, 0.4, 0.5, 0.4, 1.0, 0.4, 0.6, 0.4, 1.0, 0.5,
];
const ACID_P1: &[f32] = &[
    0.7, 0.4, 1.0, 0.4, 0.5, 0.6, 1.0, 0.4, 0.7, 0.4, 1.0, 0.4, 0.5, 0.6, 1.0, 0.4,
];
const ACID_POLES: &[&[f32]] = &[ACID_W, ACID_P1];

pub struct ClubStyle {}

impl ClubStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ClubStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for ClubStyle {
    fn name(&self) -> &str {
        "club"
    }

    fn bpm(&self) -> f32 {
        126.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Deep house кач 54–58%: свингованные 16-е вместо прямых.
        1.2
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn reverb_wet(&self) -> f32 {
        0.08
    }

    fn reverb_size(&self) -> f32 {
        0.3
    }

    /// Срез верхов мастера, Гц. Тёмный аналоговый верх: полка выше
    /// 16 кГц срезана (спека deep house). Было 19000 — лишний воздух цифры.
    fn treble_cutoff(&self) -> f32 {
        16000.0
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Deep house: биты, ударные, бас, сквозной плак темы,
        // Rhodes-стабы и шейкер. Без синтезаторных верхов
        // и шумовых подложек.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Pluck,
            Slot::Keys,
            Slot::Shaker,
        ]
    }

    /// Гости deep house: соул-чопы или кислота по одной-две,
    /// состав зависит от сида. Чопы чаще, кислота — специя.
    fn flex_pool(&self) -> &'static [Slot] {
        &[Slot::Vocal, Slot::Acid]
    }

    fn flex_weights(&self) -> &'static [f32] {
        &[1.0, 0.5]
    }

    fn flex_range(&self) -> (usize, usize) {
        (1, 2)
    }

    fn flex_count(&self) -> usize {
        1
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 2.2,
            duration_mult: 1.8,
            vel_base: 0.78,
            vel_energy: 0.25,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            octave_up: 0.15,
            top_pitch: 50,
            vel_high_cut: 0.6,
            bass_fifth: true,
            // Глубокий ровный саб вместо струны: презенс-цепь придавлена,
            // фундамент — чистый синус. Рок держит струну (0.0).
            // Громкость с запасом: саб без презенса тише струнного.
            sub_bias: 0.8,
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, 1.8, 0.78)
        };
        let kick = RoleGuide {
            // Заливка перед дропом умеренная: пулемет вместо брейка
            // ломает бит (связка с даунбитом).
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(FOUR_FLOOR, 36, 0.95)
        };
        let pad = RoleGuide {
            duration_mult: 8.0,
            vel_base: 0.5,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, 8.0, 0.5)
        };
        let snare = RoleGuide {
            vel_base: 0.95,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(CLAP_W, 38, 0.95)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            drop_before_climax: true,
            long_offbeats: true,
            long_mult: 5.0,
            duration_mult: 0.5,
            vel_base: 0.65,
            vel_energy: 0.35,
            vel_jitter: 0.1,
            poles: Some(HAT_POLES),
            // Первый emergent-слот: хэт бьет собственным пульсом из сида
            // (фэйзинг вместо паттерна), веса/полюса — запасной путь.
            emergent_beats: 0.45,
            ..RoleGuide::kit(HAT_W, 42, 0.65)
        };
        // Сквозная тема битмейкера: плак ведет мотив сида через секции —
        // у бита есть мелодическое лицо, а не только грув. Гости (клавиши,
        // чопы) — текстура вокруг темы, а не замена ей.
        let pluck = RoleGuide {
            energy_response: 1.5,
            duration_mult: 0.9,
            vel_base: 0.45,
            vel_energy: 0.3,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 4,
            },
            follow_motif: true,
            ..RoleGuide::pitched(LEAD_W, 0, 2, 4, 0.9, 0.45)
        };
        let keys = RoleGuide {
            // Гость-текучка, а не рубильник: ровное присутствие
            // через секции вместо «то нет, то стена».
            energy_response: 1.2,
            duration_mult: 1.5,
            vel_base: 0.55,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 2, 1, 0, 1.5, 0.55)
        };
        // 808 следует за баслайном (слайды между фразами), тот же four-floor.
        // Перед дропом — саб-ролл: удар вместо звона тарелки.
        let kick808 = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            ..RoleGuide::pitched(FOUR_FLOOR, 0, -1, 0, 1.0, 0.95)
        };
        let laser = RoleGuide {
            energy_response: 2.0,
            duration_mult: 1.0,
            vel_base: 0.5,
            vel_energy: 0.3,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            follow_motif: true,
            ..RoleGuide::pitched(LEAD_W, 0, 2, 2, 1.0, 0.5)
        };
        // Вокальные чопы из deep-наследия: задранные, короткие, в эхе.
        // Только для ручного включения: в битмейкере вокала нет.
        // Текут через секции, а не мигают (отклик сглажен).
        let vocal = RoleGuide {
            energy_response: 1.2,
            duration_mult: 1.6,
            vel_base: 0.55,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 2,
                jitter: 3,
            },
            ..RoleGuide::pitched(VOCAL_W, 2, 2, 3, 1.6, 0.55)
        };
        // Чужие слоты: рабочие, но сдержанные направляющие — включаются
        // только ручкой фронта, характер стиля не ломают.
        let vinyl = RoleGuide {
            duration_mult: 200.0,
            vel_base: 0.4,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 200.0, 0.4)
        };
        let flute = RoleGuide {
            energy_response: 1.2,
            duration_mult: 4.0,
            vel_base: 0.45,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 3,
            },
            ..RoleGuide::pitched(LEAD_W, 0, 1, 3, 4.0, 0.45)
        };
        let festival = RoleGuide {
            energy_response: 2.2,
            duration_mult: 0.8,
            vel_base: 0.5,
            vel_energy: 0.3,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 2,
            },
            follow_motif: true,
            ..RoleGuide::pitched(LEAD_W, 0, 2, 2, 0.8, 0.5)
        };
        // Чужие мелодические/текстуры: разреженные направляющие.
        let guitar = RoleGuide {
            energy_response: 1.2,
            duration_mult: 1.5,
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 3,
            },
            ..RoleGuide::pitched(LEAD_W, 0, 1, 3, 1.5, 0.4)
        };
        let rain = RoleGuide {
            duration_mult: 150.0,
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, 150.0, 0.35)
        };
        // Фазз вне рок-полки: тихий редкий рифф для ручки фронта.
        let fuzz = RoleGuide {
            energy_response: 1.0,
            duration_mult: 1.5,
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, 1.5, 0.35)
        };
        // Кислотный бас поверх саба: плотная линия, сквелч с акцентами.
        // Джиттер строго 0: эйсид дублирует бас октавой выше —
        // расстроенная октава бьётся с гармониками саба (жабий стрекот).
        let acid = RoleGuide {
            energy_response: 1.6,
            duration_mult: 0.9,
            vel_base: 0.6,
            vel_energy: 0.2,
            vel_jitter: 0.07,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 0,
            },
            vel_high_cut: 0.8,
            poles: Some(ACID_POLES),
            ..RoleGuide::pitched(ACID_W, 0, 0, 1, 0.4, 0.65)
        };
        // Шейкер deep house: песок 16-х поверх хэта, акцент на оффбитах.
        let shaker = RoleGuide {
            energy_response: 1.0,
            duration_mult: 0.4,
            vel_base: 0.5,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            ..RoleGuide::kit(SHAKER_W, 60, 0.5)
        };
        [
            bass, kick, pad, snare, hat, pluck, keys, kick808, laser, vinyl, vocal, flute,
            festival, guitar, rain, fuzz, acid, shaker,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{render_style, StepContext};
    use super::*;
    use crate::playback::{HAT_ID, KICK808_ID, KICK_ID, SNARE_ID};

    fn render(seed: u64, steps: usize) -> Vec<(usize, crate::instruments::NoteEvent)> {
        // Тестируем направляющие, а не микс: активны все слоты.
        let style = ClubStyle::new();
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
    fn acid_doubles_bass_in_unison() {
        // Правило против жабьего стрекота: эйсид — октава саба
        // (та же ступень, джиттер 0), иначе фундамента бьются
        // с гармониками (замер: 11.5 Гц в полосе 120–500).
        use crate::instruments::Slot;
        let style = ClubStyle::new();
        let guides = style.slot_guides();
        let at = |slot: Slot| {
            Slot::ALL.iter().position(|s| *s == slot).map(|i| &guides[i]).unwrap()
        };
        let (bass, acid) = (at(Slot::Bass), at(Slot::Acid));
        let deg = |g: &RoleGuide| match g.voice {
            Voicing::Scale { degree, octave, jitter } => (degree, octave, jitter),
            _ => (0, 0, 0),
        };
        let (bd, bo, _) = deg(bass);
        let (ad, ao, aj) = deg(acid);
        assert_eq!((ad, aj), (bd, 0), "acid must mirror bass degree without jitter");
        assert_eq!(ao, bo + 1, "acid must sit exactly an octave above bass");
    }

    #[test]
    fn downbeat_breathes_but_holds() {
        // Якорь 1: пропуск возможен (приоритет 0.95), но даунбит держится
        // в подавляющем большинстве тактов — лицо жанра цело.
        let notes = render(11, 1024);
        let downs = 1024 / 16;
        let hits = notes
            .iter()
            .filter(|(step, n)| n.instrument_id == KICK_ID && step % 16 == 0)
            .count();
        assert!(hits * 10 >= downs * 9, "downbeat lost: {hits}/{downs}");
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
    fn kick808_is_four_on_floor() {
        // После впуска в интро (шаги 32+): four-floor держится.
        let notes = render(7, 128);
        let quarters_with_kick = notes
            .iter()
            .filter(|(step, n)| n.instrument_id == KICK808_ID && *step >= 32 && step % 4 == 0)
            .count();
        assert!(
            quarters_with_kick >= 20,
            "four-on-the-floor broken: {quarters_with_kick}"
        );
    }

    #[test]
    fn all_pitches_are_kit_or_scale() {
        for (_, n) in render(99, 64) {
            // 808-й мелодический (следует за баслайном), остальные
            // ударные — фиксированный набор.
            if [KICK_ID, SNARE_ID, HAT_ID].contains(&n.instrument_id) {
                assert!([36, 38, 42].contains(&n.midi_pitch));
            } else {
                assert!((28..=90).contains(&n.midi_pitch));
            }
        }
    }
}
