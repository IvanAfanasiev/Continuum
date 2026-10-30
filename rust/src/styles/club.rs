use super::riff::RiffKind;
use super::transition::{Kind, Spec};
use super::{RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Club: битмейкер. Прямая бочка + 808-слой, глубокий саб,
/// клэп на 2 и 4 и виниловый фон.
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
    0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0,
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
// Клэп с гостами: бэкбит держат якоря, между — едва слышный шепот.
// Громкие госты давали скачущий снейр вместо бэкбита.
const CLAP_GHOST_W: &[f32] = &[
    0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.2,
];
const SNARE_POLES: &[&[f32]] = &[CLAP_W, CLAP_GHOST_W];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const VOCAL_W: &[f32] = &[
    0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Полюса кика (выбор — сидом на весь трек): строгая четверка
// и синкопы с тихими толчками (ref: Bad Angel). Толчки — шепот
// (0.3), а не второй бит: горячие восьмые давали скачущий бит.
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.3, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.35, 0.0,
];
const KICK_DRIVE8_W: &[f32] = &[
    // Вдох перед единицей: 15-й шаг пустой всегда, иначе связка
    // 14-15-0 дает фламовую очередь вместо грува.
    1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.35, 0.0,
];
// Драйв с оффбитными толчками: четверка плюс тихая синкопа на слабых.
const KICK_PUSH_W: &[f32] = &[
    1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.3, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.0,
];
// Шейкер: песок 16-х, оффбитные восьмые — якоря (шейкер ровный),
// четверти и 16-е заполняют.
const SHAKER_W: &[f32] = &[
    0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.4,
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

    fn style_contract(&self) -> super::contract::StyleContract {
        // Club: бас уступает кику, фраза дышит velocity-аркой,
        // не больше трёх мелодических онсетов на шаг, верхний регистр 78.
        super::contract::StyleContract::new(true, true, 3, Some(78))
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    /// Форма стиля: свои длины секций — стиль должен отличаться
    /// не тембрами, а построением. Сид выбирает один из трёх рисунков.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::HOUSE_SCORE
    }

    fn arrangement_for_seed(&self, seed: u64) -> crate::arranger::Score {
        use crate::arranger::{Score, SongSection};
        const STEADY: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 8),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 12),
                (SongSection::PostChorus, 4),
            ],
        };
        const LONG_GROOVE: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 10),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 10),
                (SongSection::PostChorus, 4),
            ],
        };
        const SHORT_ARC: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 6),
                (SongSection::BuildUp, 2),
                (SongSection::Drop, 10),
                (SongSection::PostChorus, 4),
            ],
        };
        crate::arranger::pick_score(seed, 0xC1B5_F0A7, &[STEADY, LONG_GROOVE, SHORT_ARC])
    }

    /// Аранж deep house: сетка never стоит, вокал и эффекты приходят
    /// только к дропу, передышка — короткая (это club, а не half-time).
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{default_layers, L_GROOVE, L_HARMONY, L_TEXTURE, L_THEME};
        let base = default_layers();
        [
            // Тема: грув, бас, тема и Rhodes-стабы. Вокал молчит — его
            // вход на дропе слышен как событие.
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Разгон: добавляется вокал.
            base[1],
            // Удар: весь состав.
            base[2],
            // Передышка: грув и Rhodes, тема отдыхает.
            L_GROOVE | L_HARMONY | L_TEXTURE,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Диско-склейка: грув катит дальше, голос следующего куска входит
        // поверх него — стыка не слышно.
        crate::piece::Transition::Rolling { bars: 2 }
    }

    /// Club: снейр-ролл 16-ми в последнем такте разгона и короткий
    /// хвост после удара — энергичный, но не пулемет на весь такт.
    fn transition_gesture(&self) -> Option<Spec> {
        Some(Spec::new(
            Kind::SnareRoll,
            &[Slot::Snare],
            &[Slot::Snare],
            3,
            60,
        ))
    }

    fn riff_kind(&self) -> RiffKind {
        RiffKind::Club
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
        // Deep house: бочка, клэп, бас, плак темы, Rhodes-стабы и
        // шейкер. Хэта нет: звонкая тарелка на верху убивала характер
        // (в микс шл «треугольник» вместо бита) — ритм держит шейкер.
        // Пад и вокал — краски палитры: одни сиды берут их, другие
        // оставляют клуб сухим.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Pluck,
            Slot::Keys,
            Slot::Shaker,
            Slot::Pad,
            Slot::Vocal,
        ]
    }

    fn signature_slots(&self) -> &'static [Slot] {
        // Тема плаком и Rhodes-стабы — лицо club.
        &[Slot::Pluck, Slot::Keys, Slot::Shaker]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Бас ходит корнями за прогрессией (джиттер 1 — соседние
        // ступени, не прыжки): баслайн вместо дрона на одной ноте.
        // Громкость держит иерархию: транзиент кика крыть обязан —
        // сустейн баса ниже пика бочки, бит ведет кик, а не бас.
        let bass = RoleGuide {
            energy_response: 2.2,
            gates: &[1.5, 2.0, 3.0],
            tie_repeats: 0.55,
            vel_base: 0.78,
            vel_energy: 0.25,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 1,
            },
            octave_up: 0.08,
            top_pitch: 50,
            vel_high_cut: 0.6,
            bass_fifth: true,
            // Саб сидит низко: кап 43 (G2), прыжки вверх редкие.
            // Выше — уже не фундамент, а вторая мелодия.
            bass_top: 43,
            // Глубокий ровный саб вместо струны: презенс-цепь придавлена,
            // фундамент — чистый синус. Рок держит струну (0.0).
            // Громкость с запасом: саб без презенса тише струнного.
            sub_bias: 0.8,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, &[1.5, 2.0, 3.0], 0.78)
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
            gates: &[6.0, 8.0, 10.0],
            vel_base: 0.5,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, &[6.0, 8.0, 10.0], 0.5)
        };
        let snare = RoleGuide {
            vel_base: 0.95,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(CLAP_W, 60, 0.95)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            long_offbeats: true,
            // Короче и тише: звонкий металл на верху давал «треугольник»
            // вместо бита и забивал весь верх микса. Ритм держит шейкер.
            gates: &[0.35, 0.7, 1.1],
            vel_base: 0.42,
            vel_energy: 0.3,
            vel_jitter: 0.1,
            poles: Some(HAT_POLES),
            // Хэт держит сетку полюсов: собственный пульс давал фэйзинг
            // против 16-х и сбивал ритм — бит важнее дрейфа.
            // Грув хэта свой на каждую секцию (у дропа — свой бит),
            // выбор детерминирован сидом и секцией.
            section_poles: true,
            ..RoleGuide::kit(HAT_W, 42, 0.65)
        };
        // Сквозная тема битмейкера: плак ведет мотив сида через секции —
        // у бита есть мелодическое лицо, а не только грув. Ноты длинные:
        // линия льется фразами, а не тычет разовыми ударами.
        let pluck = RoleGuide {
            energy_response: 1.5,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.55,
            vel_energy: 0.3,
            vel_jitter: 0.07,
            // Унисон темы: носители поют те же ступени (джиттер 0) —
            // цельная мелодия, краски различают тембры, а не высоты.
            // Октава на единицу ниже: синтезатор на самом верху спорил
            // с EP и хэтом — линии надо держаться в середине.
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.2,
            ..RoleGuide::pitched(LEAD_W, 0, 1, 0, &[1.0, 1.5, 2.0], 0.45)
        };
        let keys = RoleGuide {
            // Гость-текучка, а не рубильник: ровное присутствие
            // через секции вместо «то нет, то стена».
            energy_response: 1.2,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.62,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 2, 1, 0, &[1.0, 1.5, 2.0], 0.55)
        };
        // 808 держит сетку four-floor корнями прогрессии (тот же
        // гармонический путь, что у саба); слайд между нотами — дело
        // голоса Kick808, генератор лишь ставит корни на доли.
        // Перед дропом — саб-ролл: удар вместо звона тарелки.
        // Регистр тот же, что у саба (фолд в mod.rs): 808 — бас, а не лид.
        let kick808 = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            bass_top: 43,
            ..RoleGuide::pitched(FOUR_FLOOR, 0, -1, 0, &[1.0], 0.95)
        };
        // Оперный голос: редкие долгие «ah→oh» в эхе, сидит сзади.
        // Только для ручного включения: в битмейкере вокала нет.
        // Текут через секции, а не мигают (отклик сглажен).
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
            ..RoleGuide::pitched(VOCAL_W, 2, 2, 3, &[2.0, 3.0, 4.0], 0.45)
        };
        // Чужие слоты: рабочие, но сдержанные направляющие — включаются
        // только ручкой фронта, характер стиля не ломают.
        let vinyl = RoleGuide {
            gates: &[200.0],
            vel_base: 0.4,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[200.0], 0.4)
        };
        // Чужие мелодические/текстуры: разреженные направляющие.
        // Гитара в club — ручной гость (ритм-гитара вместо плака).
        let guitar = RoleGuide {
            energy_response: 1.2,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 3,
            },
            ..RoleGuide::pitched(LEAD_W, 0, 1, 3, &[1.0, 1.5, 2.0], 0.4)
        };
        // Шейкер deep house: песок 16-х поверх хэта, акцент на оффбитах.
        // Дождь: ровная пелена во все фразы (тише винила и музыки).
        let rain = RoleGuide {
            gates: &[150.0],
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[150.0], 0.35)
        };
        let shaker = RoleGuide {
            energy_response: 1.0,
            gates: &[0.4],
            vel_base: 0.62,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            ..RoleGuide::kit(SHAKER_W, 60, 0.62)
        };
        // Соло-гитара вне автомикса club: ручная краска для фронта.
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 1, &[1.0, 1.5, 2.0], 0.35)
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
            // ударные — фиксированный набор. Клэп помечен высотой 60:
            // по ней снейр играет клэпом, а не барабаном.
            if [KICK_ID, SNARE_ID, HAT_ID].contains(&n.instrument_id) {
                assert!([36, 38, 42, 60].contains(&n.midi_pitch));
            } else {
                assert!((28..=90).contains(&n.midi_pitch));
            }
        }
    }
}
