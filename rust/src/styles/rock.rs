use super::riff::RiffKind;
use super::transition::{Kind, Spec};
use super::{RoleGuide, Style, StyleGroup, Voicing};
use crate::instruments::Slot;

/// Rock: гитарный концерт. Мощный рифф гитары ведёт, соло-гитара
/// отвечает, вдавленная бочка, хлесткий снейр, бас риффует
/// вслед за гитарой вплоть до D3. Никаких пианино, хоров и синдров:
/// состав — группа (гитара, бас, ударные), а не оркестр.
/// Никаких переборов, свинга и эмбиент-пелены: жестко и ровно.
const SCALE: [u8; 5] = [48, 51, 53, 55, 58];

/// Рок-ходы: квинтовый напор i–iv–v и возвраты на тонику. Пентатоника
/// индексируется 0..4; никакой джазовой гармонии — только сила.
const PROGS: [[u8; 4]; 6] = [
    [0, 3, 4, 3],
    [0, 4, 3, 4],
    [0, 3, 4, 2],
    [3, 4, 0, 4],
    [0, 2, 3, 4],
    [4, 3, 0, 3],
];
/// Рок не тянет аккорд: смена каждые 2–4 такта, riff идет следом.
const HARMONY_BARS: [usize; 2] = [2, 4];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.4, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const KICK_P2: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Двойные удары: халф-тайм с дабл-кик флером перед даунбитом
// (15-й шаг пустой — вдох перед единицей).
const KICK_DBL_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.8, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_P1, KICK_P2, KICK_DBL_W];

const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_P1: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const SNARE_POLES: &[&[f32]] = &[SNARE_W, SNARE_P1];

const HAT_W: &[f32] = &[
    0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.0, 0.7, 0.0, 0.5, 0.4,
];
const HAT_P1: &[f32] = &[
    0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4, 0.6, 0.4,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_P1];
// Шейкер: ровные восьмые, тихо.
const SHAKER_ROCK_W: &[f32] = &[
    0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.6, 0.0,
];

const BASS_W: &[f32] = &[
    1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.0, 1.0, 0.0, 0.6, 0.5,
];
const BASS_P1: &[f32] = &[
    1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.0, 0.6, 0.0, 0.0, 1.0, 0.0, 0.6, 0.0,
];
const BASS_POLES: &[&[f32]] = &[BASS_W, BASS_P1];

// Рев: рифф чопами по мотиву сида.
const RIFF_W: &[f32] = &[
    0.8, 0.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.0, 0.8, 0.0, 0.6, 0.6,
];
const RIFF_P1: &[f32] = &[
    0.8, 0.0, 0.0, 0.6, 0.0, 0.8, 0.0, 0.0, 0.6, 0.0, 0.8, 0.0, 0.0, 0.6, 0.8, 0.0,
];
const RIFF_POLES: &[&[f32]] = &[RIFF_W, RIFF_P1, RIFF16_W];
// Пулеметные 16-е медиатором (ref: Thunderstruck): скорый перебор
// по ступеням темы. Общий полюс риффа: фраза решает, чоппы или пулемет,
// исполнители (фазз, гитара) играют ее каждый своим тембром.
const RIFF16_W: &[f32] = &[
    0.6, 0.5, 0.7, 0.5, 0.6, 0.5, 0.7, 0.5, 0.6, 0.5, 0.7, 0.5, 0.6, 0.5, 0.7, 0.6,
];
// Соло-скелет: редкие длинные доли (перебор), а не чоппы риффа.
// Тот же мотив, другой ритм — соло отвечает риффу.
const ROCK_LEAD_W: &[f32] = &[
    0.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.6, 0.0,
];

const BED_W: &[f32] = &[
    0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

pub struct RockStyle {}

impl RockStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RockStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for RockStyle {
    fn name(&self) -> &str {
        "rock"
    }

    fn group(&self) -> StyleGroup {
        StyleGroup::Rock
    }

    fn bpm(&self) -> f32 {
        130.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        1.0
    }

    fn style_contract(&self) -> super::contract::StyleContract {
        // Рок: бас часто идёт вместе с киком — это лицо стиля, уступает
        // только по мелодической плотности. Не больше двух онсетов на шаг.
        super::contract::StyleContract::new(false, true, 2, None)
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn progressions(&self) -> &'static [[u8; 4]] {
        &PROGS
    }

    fn harmonic_rhythm(&self) -> &'static [usize] {
        &HARMONY_BARS
    }

    fn key_shift(&self) -> i8 {
        // E-минор: гитарный строй, ниже клубного C по характеру.
        4
    }

    fn density(&self) -> f32 {
        // Рок гуще: рифф и бас идут вместе с бочкой.
        1.12
    }

    /// Форма стиля: свои длины секций — стиль должен отличаться
    /// не тембрами, а построением.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::ROCK_SCORE
    }

    fn arrangement_for_seed(&self, seed: u64) -> crate::arranger::Score {
        use crate::arranger::{Score, SongSection};
        const LONG_DROP: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 10),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 12),
                (SongSection::PostChorus, 6),
            ],
        };
        const WIDE: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 8),
                (SongSection::BuildUp, 6),
                (SongSection::Drop, 10),
                (SongSection::PostChorus, 6),
            ],
        };
        crate::arranger::pick_score(
            seed,
            0x0B0C_F0A7,
            &[crate::arranger::ROCK_SCORE, LONG_DROP, WIDE],
        )
    }

    /// Аранж рока: рифф (тема) не уходит даже в передышку — это держащая
    /// линия группы. Соло-гитара и клавиши приходят к дропу.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{default_layers, L_GROOVE, L_HARMONY, L_THEME};
        let base = default_layers();
        [
            // Тема: грув, бас и рифф. Клавиши в теме не нужны.
            L_GROOVE | L_THEME,
            // Разгон: подключаются клавиши.
            L_GROOVE | L_HARMONY | L_THEME,
            // Удар: весь состав.
            base[2],
            // Передышка: рифф остается, клавиши уходят.
            L_GROOVE | L_THEME,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Рок стыкуется резко: короткий хвост с ударом, дальше следующий
        // кусок сразу с полной энергией.
        crate::piece::Transition::Cut
    }

    /// Rock: филл бочка/снейр как томы в последнем такте разгона.
    /// Хвост — только снейр: даунбит дропа остается за бочкой.
    fn transition_gesture(&self) -> Option<Spec> {
        Some(Spec::new(
            Kind::TomFill,
            &[Slot::Kick, Slot::Snare],
            &[Slot::Snare],
            3,
            36,
        ))
    }

    fn riff_kind(&self) -> RiffKind {
        RiffKind::Rock
    }

    fn reverb_wet(&self) -> f32 {
        0.08
    }

    fn reverb_size(&self) -> f32 {
        0.3
    }

    fn treble_cutoff(&self) -> f32 {
        18500.0
    }

    /// Форма энергии: пресс держит весь круг, дроп — пик пресса.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.55 + 0.45 * energy
    }

    /// Сайдчейн давит агрессивно: весь микс качает под прямой ритм.
    fn sidechain_depth(&self) -> f32 {
        1.5
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Группа, а не оркестр: бочка, снейр, хэт, бас, гитара риффом
        // и соло-гитара перебором. Клавиши — редкая краска палитры:
        // сид решает, добавить ли орган к дропу. Без тарелок и текстур:
        // металлический звон не к месту даже на стыках. Фазз — вручную.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Guitar,
            Slot::GuitarLead,
            Slot::Keys,
        ]
    }

    fn signature_slots(&self) -> &'static [Slot] {
        // Риффовая гитара — лицо рока; соло и орган — краски сида.
        &[Slot::Guitar]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        let bass = RoleGuide {
            energy_response: 1.4,
            gates: &[0.4, 0.6, 0.9],
            tie_repeats: 0.3,
            vel_base: 0.8,
            vel_energy: 0.15,
            vel_jitter: 0.04,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            top_pitch: 52,
            vel_high_cut: 0.6,
            poles: Some(BASS_POLES),
            // Бас риффует вслед за гитарой (ref: doubling вплоть до D3),
            // а не лежит сабом: кап выше.
            bass_top: 52,
            ..RoleGuide::pitched(BASS_W, 0, -1, 0, &[0.4, 0.6, 0.9], 0.8)
        };
        // Вдавленная бочка: якоря в пол, компрессия — в миксе.
        // Заливка умеренная, чтобы не ломать бит очередью.
        let kick = RoleGuide {
            vel_base: 1.0,
            vel_energy: 0.0,
            vel_jitter: 0.02,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 1.0)
        };
        let pad = RoleGuide {
            gates: &[6.0, 8.0, 10.0],
            vel_base: 0.25,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[6.0, 8.0, 10.0], 0.25)
        };
        // Хлесткий снейр: транзиент + тело.
        let snare = RoleGuide {
            vel_base: 0.9,
            vel_jitter: 0.04,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(SNARE_W, 38, 0.9)
        };
        let hat = RoleGuide {
            energy_response: 1.0,
            // Рок-хэт — закрытый: открытый хэт тут не был частью грува.
            gates: &[0.35, 0.7],
            vel_base: 0.42,
            vel_energy: 0.18,
            vel_jitter: 0.06,
            poles: Some(HAT_POLES),
            // Хэт держит сетку полюсов: пульс вместо сетки сбивал ритм.
            // Грув свой на секцию: у дропа свой бит.
            section_poles: true,
            ..RoleGuide::kit(HAT_W, 42, 0.42)
        };
        let pluck = RoleGuide {
            energy_response: 1.0,
            gates: &[0.6, 0.8, 1.2],
            vel_base: 0.28,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 1, 2, &[0.6, 0.8, 1.2], 0.28)
        };
        let keys = RoleGuide {
            energy_response: 1.0,
            gates: &[2.0, 3.0, 4.0],
            vel_base: 0.28,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 0, &[2.0, 3.0, 4.0], 0.28)
        };
        // Чужие слоты: сдержанные направляющие для ручек фронта.
        let kick808 = RoleGuide {
            vel_base: 0.7,
            ..RoleGuide::kit(KICK_W, 36, 0.7)
        };
        let vinyl = RoleGuide {
            gates: &[200.0],
            vel_base: 0.2,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[200.0], 0.2)
        };
        let vocal = RoleGuide {
            energy_response: 1.0,
            gates: &[0.9, 1.2, 1.6],
            vel_base: 0.3,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(RIFF_W, 0, 1, 2, &[0.9, 1.2, 1.6], 0.3)
        };
        // Чистая гитара риффом по мотиву: общий скелет риффа, своим
        // тембром и броском гейта.
        // Чоппирующие восьмые поверх рева, без перегруза.
        // Второй полюс — пулеметные 16-е медиатором (ref: Thunderstruck).
        // Ведёт без отдыха: рифф relentless, передышка — у стены риффа.
        let guitar = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0],
            vel_base: 0.85,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 1,
            },
            follow_motif: true,
            phrase_rest: 0.0,
            poles: Some(RIFF_POLES),
            ..RoleGuide::pitched(RIFF_W, 0, 1, 1, &[1.0], 0.85)
        };
        // Соло поверх риффа: поёт тему редко и длинно (перебор),
        // а не чоппит вместе с риффом. Другой ритм (редкие доли),
        // та же тема — вопрос-ответ с риффом, а не унисон.
        // Октава та же, что у риффа (вверх — писк, вниз — каша с басом).
        let guitar_lead = RoleGuide {
            energy_response: 1.5,
            gates: &[1.5, 2.0, 3.0, 4.0],
            vel_base: 0.7,
            vel_energy: 0.2,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.45,
            ..RoleGuide::pitched(ROCK_LEAD_W, 0, 1, 0, &[1.5, 2.0, 3.0, 4.0], 0.7)
        };
        // Шейкер вне club: тихий ровный слой для ручки фронта.
        // Дождь: ровная пелена во все фразы (тише винила и музыки).
        let rain = RoleGuide {
            gates: &[150.0],
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[150.0], 0.35)
        };
        let shaker = RoleGuide {
            energy_response: 1.0,
            gates: &[0.4],
            vel_base: 0.3,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SHAKER_ROCK_W, 60, 0.3)
        };
        // Формула вне club: редкая математическая специя для ручки фронта.
        // Вейвтейбл вне автомикса: ручная краска для фронта.
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
    use super::*;
    use crate::instruments::Slot;

    #[test]
    fn rock_keeps_its_core_band() {
        // Ядро рока — ударные, бас и риффовая гитара — не зависит от сида.
        // Соло-гитара, клавиши и хэт-краски приходят по касту: сид решает,
        // нужен ли второй голос, но группу не раздувает до оркестра.
        let style = RockStyle::new();
        let palette = style.ensemble();
        for slot in [
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Hat,
            Slot::Guitar,
            Slot::GuitarLead,
            Slot::Keys,
        ] {
            assert!(palette.contains(&slot), "{slot:?} missing from palette");
        }
        assert_eq!(palette.len(), 7, "palette bloated: {palette:?}");
        for seed in [1u64, 2, 3, 42, 777] {
            let cast = style.cast(seed);
            for slot in [Slot::Bass, Slot::Kick, Slot::Snare, Slot::Hat, Slot::Guitar] {
                assert!(cast.contains(&slot), "seed {seed}: {slot:?} lost");
            }
            assert!(
                cast.len() <= palette.len(),
                "seed {seed}: cast outgrew palette"
            );
        }
        // Гитара ведёт без отдыха и в полную (рельеф — у стены фазза).
        let guides = style.slot_guides();
        let gi = Slot::ALL.iter().position(|s| *s == Slot::Guitar).unwrap();
        assert_eq!(guides[gi].phrase_rest, 0.0, "riff must not rest");
        assert!(
            guides[gi].vel_base >= 0.85,
            "riff too quiet: {}",
            guides[gi].vel_base
        );
    }
}
