use super::{
    dsp::{release_fade, saw_blep, square_blep, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Кислотный бас в духе TB-303: пила через резонансный lowpass
/// с огибающей среза, акценты (громкие ноты — ярче и злее) и слайды
/// между нотами. Сквигл поверх саба, а не вместо него.
const RESO: f32 = 3.0;
const CUTOFF_BASE_HZ: f32 = 500.0;
const CUTOFF_ENV_HZ: f32 = 2000.0;
const ENV_DECAY: f32 = 9.0;
const ACCENT_THRESH: f32 = 0.7;
const ACCENT_BOOST: f32 = 1.35;
const GLIDE_SECS: f32 = 0.035;
const ATTACK_SECS: f32 = 0.002;
const VOICE_GAIN: f32 = 0.34;
const MAX_RING_SECS: f32 = 2.0;
const FADE_SECS: f32 = 0.015;

#[derive(Clone, Default)]
struct AcidVoice {
    sample_rate: f32,
    phase_saw: f32,
    phase_sub: f32,
    filter: Svf,
    frequency: f32,
    base_smooth: f32,
    glide_coeff: f32,
    gain: f32,
    accent: bool,
    t: f32,
    samples_left: usize,
}

impl Voice for AcidVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
        self.glide_coeff = 1.0 - (-1.0 / (self.sample_rate * GLIDE_SECS)).exp();
    }

    fn set_glide_start(&mut self, freq_hz: f32) {
        // Слайд стартует с высоты прошлой ноты (ставит обертка).
        self.base_smooth = freq_hz.max(30.0);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.accent = note.velocity >= ACCENT_THRESH;
        self.t = 0.0;
        self.phase_saw = 0.0;
        self.phase_sub = 0.0;
        self.filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        // Высота ползет к ноте (слайд), фильтр открывается акцентом
        // и гаснет за ~100 мс — фирменный сквелч.
        self.base_smooth += (self.frequency - self.base_smooth) * self.glide_coeff;
        let env = (-ENV_DECAY * self.t).exp();
        let cutoff = CUTOFF_BASE_HZ
            + CUTOFF_ENV_HZ * env * if self.accent { 1.0 } else { 0.45 };

        self.phase_saw = (self.phase_saw + self.base_smooth / sample_rate) % 1.0;
        self.phase_sub = (self.phase_sub + (self.base_smooth * 0.5) / sample_rate) % 1.0;
        let saw = saw_blep(self.phase_saw, self.base_smooth, sample_rate);
        let sub = square_blep(self.phase_sub, self.base_smooth * 0.5, sample_rate) * 0.4;
        let (low, _, _) = self.filter.process(saw + sub, cutoff, RESO, sample_rate);

        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);
        let boost = if self.accent { ACCENT_BOOST } else { 1.0 };

        self.t += 1.0 / sample_rate;
        (low * 1.2).tanh() * self.gain * boost * attack * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.filter.reset();
    }
}

/// Публичный инструмент: моно-голос (кислотные линии монофоничны)
/// + слайды между нотами. Резонанс держит SVF: безусловно стабилен.
pub struct Acid(Mono<AcidVoice>, f32);

impl Acid {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Bass), 0.0)
    }
}

impl Instrument for Acid {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.0.set_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let target = midi_to_freq(note.midi_pitch);
        let from = self.1;
        self.0.trigger_with_setup(note, |v| v.set_glide_start(from));
        self.1 = target;
    }

    fn next_sample(&mut self) -> f32 {
        self.0.next_sample()
    }

    fn id(&self) -> usize {
        self.0.id()
    }

    fn role(&self) -> InstrumentRole {
        self.0.role()
    }

    fn reset(&mut self) {
        self.0.reset();
        self.1 = 0.0;
    }

    fn is_active(&self) -> bool {
        self.0.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acid() -> Acid {
        let mut a = Acid::new(18);
        a.set_sample_rate(44_100.0);
        a
    }

    #[test]
    fn acid_squelches_finite_and_audible() {
        let mut a = acid();
        a.trigger(&NoteEvent::new(18, 45, 0.9, 0.4));
        let mut peak: f32 = 0.0;
        for _ in 0..20_000 {
            let s = a.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.12, "acid too quiet: {peak}");
        assert!(peak < 0.7, "acid blows up: {peak}");
        assert!(!a.is_active());
    }

    #[test]
    fn accent_is_brighter_than_soft() {
        // Акцент (vel ≥ 0.7) открывает фильтр шире: пик выше.
        let render = |vel: f32| {
            let mut a = acid();
            a.trigger(&NoteEvent::new(18, 45, vel, 0.4));
            let mut peak: f32 = 0.0;
            for _ in 0..20_000 {
                peak = peak.max(a.next_sample().abs());
            }
            peak
        };
        assert!(
            render(0.95) > render(0.5) * 1.2,
            "accent does not open filter"
        );
    }

    #[test]
    fn slides_between_notes_without_step() {
        // Слайд 45 → 48: высота ползет, ступеньки нет.
        let mut a = acid();
        a.trigger(&NoteEvent::new(18, 45, 0.8, 0.5));
        for _ in 0..5000 {
            a.next_sample();
        }
        a.trigger(&NoteEvent::new(18, 48, 0.8, 0.5));
        let mut prev = a.next_sample();
        assert!(prev.is_finite());
        let mut max_step: f32 = 0.0;
        for _ in 0..500 {
            let s = a.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
        }
        assert!(max_step < 0.5, "glide step: {max_step}");
    }
}
