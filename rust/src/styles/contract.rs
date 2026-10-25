//! Контракт ансамбля стиля.
//!
//! Стиль задаёт не независимые вероятности шагов, а жёсткие отношения
//! между ролями, бюджет плотности и обязательную вариацию исполнения.
//! Расширяться будет по мере переноса стилей на контракт.

/// Контракт ансамбля для одного стиля.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleContract {
    /// Бас не атакует на шаге, где играет кик: уступает фундаменту.
    pub bass_avoids_kick: bool,
    /// Фраза получает лёгкую velocity-арку: растёт к концу и выдыхает.
    pub phrase_velocity_arc: bool,
    /// Максимум мелодических онсетов на один шаг. 0 = без ограничения.
    /// Держит слои различимыми: насыщенность — контраст, а не стена нот.
    pub max_melodic_onsets_per_step: usize,
    /// Верхняя граница регистра для мелодических ролей (MIDI). None = без
    /// ограничения. Складывает уползшие ноты октавой вниз, чтобы слои не
    /// лезли в одну полосу и не было высокочастотного мусора.
    pub melodic_ceiling: Option<u8>,
}

impl StyleContract {
    pub const fn new(
        bass_avoids_kick: bool,
        phrase_velocity_arc: bool,
        max_melodic_onsets_per_step: usize,
        melodic_ceiling: Option<u8>,
    ) -> Self {
        Self {
            bass_avoids_kick,
            phrase_velocity_arc,
            max_melodic_onsets_per_step,
            melodic_ceiling,
        }
    }
}
