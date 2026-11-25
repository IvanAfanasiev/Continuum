//! Piano: ported from `continuum/backend/src/instruments.rs`
//! to this engine's architecture (`Voice` + `NoteEvent` + `VoicePool`).
//!
//! The original timbre is preserved: three slightly detuned strings per note,
//! 9 inharmonic partials with different decay rates,
//! a hammer strike (felt + wood + noise) and a soundboard filter.
//! ADSR from the original: attack 9 ms, decay 820 ms → sustain 0.18,
//! release 1100 ms — now built from the shared `ExpDecay` primitive.
//! Filters use the shared `Svf` like the other voices of this engine.

use super::{
    dsp::{note_seed, release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};

const ATTACK_SECS: f32 = 0.009;
const DECAY_SECS: f32 = 0.82;
const SUSTAIN: f32 = 0.18;
const RELEASE_SECS: f32 = 1.1;
/// Brightness of the hammer strike fades quickly after the attack.
const BRIGHTNESS_RATE: f32 = 22.0;
const FILTER_Q: f32 = 0.7;
const SOUNDBOARD_HP_HZ: f32 = 34.0;

/// (amplitude, decay rate) — as in the original synth_piano.
const PARTIALS: [(f32, f32); 9] = [
    (1.00, 0.58),
    (0.50, 0.95),
    (0.30, 1.55),
    (0.17, 2.65),
    (0.085, 4.65),
    (0.045, 7.20),
    (0.024, 10.40),
    (0.012, 14.60),
    (0.006, 19.50),
];
const UNISON: [f32; 3] = [0.46, 0.31, 0.23];
const VOICE_GAIN: f32 = 0.42;

#[derive(Clone)]
pub(crate) struct PianoVoice {
    sample_rate: f32,
    /// Phases of the three strings (0..1).
    phase: [f32; 3],
    /// Time since note onset.
    t: f32,
    gain: f32,
    detune: f32,
    inharmonicity: f32,
    freq_decay: f32,
    frequency: f32,
    samples_left: usize,
    release_samples: usize,
    /// Soundboard: low-pass with a dynamic cutoff, then a 34 Hz high-pass.
    tone_lp: Svf,
    body_hp: Svf,
    /// Recurrent envelopes instead of per-sample `exp()`.
    body_env: ExpDecay,
    brightness_env: ExpDecay,
    soundboard_env: ExpDecay,
    partial_envs: [ExpDecay; 9],
    noise: Noise,
}

impl Default for PianoVoice {
    fn default() -> Self {
        Self {
            sample_rate: 44_100.0,
            phase: [0.0, 0.137, 0.271],
            t: 0.0,
            gain: 0.0,
            detune: 0.0,
            inharmonicity: 0.0,
            freq_decay: 1.0,
            frequency: 0.0,
            samples_left: 0,
            release_samples: 0,
            tone_lp: Svf::default(),
            body_hp: Svf::default(),
            body_env: ExpDecay::default(),
            brightness_env: ExpDecay::default(),
            soundboard_env: ExpDecay::default(),
            partial_envs: std::array::from_fn(|_| ExpDecay::default()),
            noise: Noise::default(),
        }
    }
}

impl PianoVoice {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Smoothstep of the 9 ms hammer attack.
    fn attack(&self) -> f32 {
        let a = (self.t / ATTACK_SECS).min(1.0);
        a * a * (3.0 - 2.0 * a)
    }
}

impl Voice for PianoVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let sr = self.sample_rate;
        self.frequency = crate::core::midi_to_freq(note.midi_pitch);
        self.detune = (0.00026 + self.frequency * 0.00000018).clamp(0.00024, 0.00062);
        self.inharmonicity =
            (0.000045 + self.frequency * 0.00000008).clamp(0.00004, 0.00016);
        self.freq_decay = (self.frequency / 330.0).sqrt().clamp(0.72, 1.45);
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase = [0.0, 0.137, 0.271];
        self.tone_lp.reset();
        self.body_hp.reset();
        self.body_env = ExpDecay::start(1.0 / DECAY_SECS, sr);
        self.brightness_env = ExpDecay::start(BRIGHTNESS_RATE, sr);
        self.soundboard_env = ExpDecay::start(2.0, sr);
        for (i, &(_, decay_rate)) in PARTIALS.iter().enumerate() {
            self.partial_envs[i] =
                ExpDecay::start(decay_rate * self.freq_decay, sr);
        }
        self.release_samples = (RELEASE_SECS * sr) as usize;
        self.samples_left =
            (note.duration_secs.max(0.05) * sr) as usize + self.release_samples;
        self.noise = Noise::new(note_seed(note, 0xF1A0));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;
        let sr = self.sample_rate;
        let freq = self.frequency;

        self.phase[0] = (self.phase[0] + freq / sr) % 1.0;
        self.phase[1] = (self.phase[1] + freq * (1.0 + self.detune) / sr) % 1.0;
        self.phase[2] = (self.phase[2] + freq * (1.0 - self.detune * 0.72) / sr) % 1.0;

        let attack_brightness = self.brightness_env.next();
        let mut strings = 0.0f32;
        for (i, &(amp, _)) in PARTIALS.iter().enumerate() {
            let harmonic = (i + 1) as f32;
            let stretch = harmonic * (1.0 + self.inharmonicity * harmonic * harmonic);
            let decay = self.partial_envs[i].next();
            let lift = if i >= 3 {
                1.0 + attack_brightness * 0.46
            } else {
                1.0
            };
            let unison = sin01(self.phase[0] * stretch) * UNISON[0]
                + sin01(self.phase[1] * stretch) * UNISON[1]
                + sin01(self.phase[2] * stretch) * UNISON[2];
            strings += amp * decay * lift * unison;
        }

        let mut transient = 0.0f32;
        if self.t < 0.008 {
            let env = (1.0 - self.t / 0.008).powi(2);
            let felt = sin01((self.t * 1180.0) % 1.0) * 0.004;
            let wood = sin01((self.t * 92.0) % 1.0) * 0.012;
            transient = (self.noise.sample() * 0.006 + felt + wood) * env;
        }
        if self.t < 0.035 {
            transient += sin01((self.t * 48.0) % 1.0)
                * (1.0 - self.t / 0.035).powi(2)
                * 0.008;
        }

        let soundboard = strings * (0.74 + 0.08 * self.soundboard_env.next());
        let raw = (soundboard + transient) * 0.36;
        let brightness = ((freq - 180.0) / 720.0).clamp(0.0, 1.0);
        let cutoff = 1450.0 + brightness * 650.0 + attack_brightness * 2300.0;
        let (low, _, _) = self.tone_lp.process(raw, cutoff, FILTER_Q, sr);
        let (_, _, high) = self.body_hp.process(low, SOUNDBOARD_HP_HZ, FILTER_Q, sr);

        let body_level = SUSTAIN + (1.0 - SUSTAIN) * self.body_env.next();
        let release = if self.samples_left <= self.release_samples {
            release_fade(self.samples_left, self.release_samples.max(1))
        } else {
            1.0
        };
        let out = high * 0.86 * body_level * self.gain * self.attack() * release * VOICE_GAIN;

        self.t += 1.0 / sr;
        out
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.gain = 0.0;
        self.t = 0.0;
        self.tone_lp.reset();
        self.body_hp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piano_has_a_struck_spectrum_and_decays() {
        let sr = 44_100.0;
        let mut v = PianoVoice::new();
        v.set_sample_rate(sr);
        v.trigger(&NoteEvent::new(23, 57, 0.9, 1.5));
        let out: Vec<f32> = (0..(3.0 * sr) as usize).map(|_| v.next_sample()).collect();
        let rms = |lo: usize, hi: usize| {
            (out[lo..hi].iter().map(|s| s * s).sum::<f32>() / (hi - lo) as f32).sqrt()
        };
        let early = rms(0, 2205);
        let late = rms((2.0 * sr) as usize, (2.5 * sr) as usize);
        assert!(early > 0.001, "piano silent: {early}");
        assert!(late < early * 0.5, "piano does not decay: {early} vs {late}");
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(!v.is_active());
    }
}
