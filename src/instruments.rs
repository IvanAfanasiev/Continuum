// Instrument synthesis + default ADSR envelopes.
//
// Each instrument has a default EnvelopeConfig that reflects its
// natural acoustic behaviour. LayerConfig in markov.rs can override
// any field for style-specific shaping.
//
// Amplitude shaping (ADSR) is applied by Voice in audio_engine.rs.
// Instruments here focus purely on timbre.

use std::f32::consts::PI;

// ─────────────────────────────────────────────────────────────
//  ADSR ENVELOPE CONFIG
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct EnvelopeConfig {
    pub attack_ms:  f32,  // rise time from 0 to peak
    pub decay_ms:   f32,  // fall from peak to sustain level
    pub sustain:    f32,  // sustain level (0.0-1.0 of peak)
    pub release_ms: f32,  // fall from sustain to 0 after note ends
}

impl EnvelopeConfig {
    pub const fn new(attack_ms: f32, decay_ms: f32, sustain: f32, release_ms: f32) -> Self {
        Self { attack_ms, decay_ms, sustain, release_ms }
    }
}

// ─────────────────────────────────────────────────────────────
//  INSTRUMENT ENUM + DEFAULT ENVELOPES
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Instrument {
    Sine,
    Piano,
    Pluck,
    Pad,
    Bass,
    Organ,
    Kick,
    Hihat,
    Snare,
}

impl Instrument {
    // Default ADSR for this instrument.
    // These reflect the natural acoustic behaviour and sound good
    // without any LayerConfig override.
    pub fn default_envelope(self) -> EnvelopeConfig {
        match self {
            // Piano: near-instant attack, no decay (harmonics decay internally),
            // full sustain, medium release tail.
            Self::Piano => EnvelopeConfig::new(4.0, 0.0, 1.0, 600.0),

            // Pluck: instant attack, fast decay (string damps naturally),
            // low sustain (most energy in transient), short release.
            Self::Pluck => EnvelopeConfig::new(1.0, 300.0, 0.15, 80.0),

            // Pad: very slow attack (the defining pad character),
            // no decay, full sustain, slow fade-out.
            Self::Pad => EnvelopeConfig::new(700.0, 0.0, 1.0, 1200.0),

            // Bass: fast attack, slight punch-decay to sustain level,
            // medium sustain, clean release.
            Self::Bass => EnvelopeConfig::new(12.0, 80.0, 0.80, 200.0),

            // Organ: fast attack (key = air valve), no decay,
            // full sustain, very fast cut-off (organ stops abruptly).
            Self::Organ => EnvelopeConfig::new(15.0, 0.0, 1.0, 25.0),

            // Sine: smooth attack, no decay, full sustain, medium release.
            Self::Sine => EnvelopeConfig::new(20.0, 0.0, 1.0, 2500.0),

            // Kick: instant attack, fast decay (all in the transient),
            // zero sustain, very short release.
            Self::Kick => EnvelopeConfig::new(1.0, 180.0, 0.0, 30.0),

            // Hihat: instant attack, very fast decay (cymbal shimmer),
            // zero sustain.
            Self::Hihat => EnvelopeConfig::new(1.0, 60.0, 0.0, 20.0),

            // Snare: instant attack, medium decay (body + rattle),
            // zero sustain.
            Self::Snare => EnvelopeConfig::new(1.0, 120.0, 0.0, 25.0),
        }
    }

    pub fn is_percussion(self) -> bool {
        matches!(self, Self::Kick | Self::Hihat | Self::Snare)
    }
}

impl Default for Instrument {
    fn default() -> Self { Self::Sine }
}

// ─────────────────────────────────────────────────────────────
//  INSTRUMENT STATE
// ─────────────────────────────────────────────────────────────

pub struct InstrumentState {
    pub phase:     f32,
    pub phase2:    f32,
    pub phase3:    f32,
    pub t:         f32,
    noise_seed:    u32,
    flutter_phase: f32,
    kick_freq:     f32,
    ks_buf: [f32; 2048],
    ks_pos: usize,
    ks_len: usize,
}

impl InstrumentState {
    pub fn new() -> Self {
        Self {
            phase: 0.0, phase2: 0.137, phase3: 0.271,
            t: 0.0,
            noise_seed:    12345,
            flutter_phase: 0.0,
            kick_freq:     80.0,
            ks_buf: [0.0; 2048],
            ks_pos: 0,
            ks_len: 100,
        }
    }

    pub fn reset(&mut self, freq: f32, sample_rate: f32, instrument: Instrument) {
        self.t = 0.0;
        match instrument {
            Instrument::Pluck => {
                let len = (sample_rate / freq.max(1.0)).round() as usize;
                self.ks_len = len.clamp(2, 2047);
                self.ks_pos = 0;
                for i in 0..self.ks_len {
                    self.ks_buf[i] = self.white_noise() * 0.9;
                }
            }
            Instrument::Kick => {
                self.phase     = 0.0;
                self.kick_freq = freq * 3.5;
            }
            Instrument::Hihat | Instrument::Snare => {
                self.phase  = 0.0;
                self.phase2 = 0.0;
            }
            _ => {}
        }
    }

    pub fn next_sample(
        &mut self,
        instrument:  Instrument,
        freq:        f32,
        amplitude:   f32,
        sample_rate: f32,
    ) -> f32 {
        self.t += 1.0 / sample_rate;
        let raw = match instrument {
            Instrument::Sine  => self.synth_sine(freq, sample_rate),
            Instrument::Piano => self.synth_piano(freq, sample_rate),
            Instrument::Pluck => self.synth_pluck(),
            Instrument::Pad   => self.synth_pad(freq, sample_rate),
            Instrument::Bass  => self.synth_bass(freq, sample_rate),
            Instrument::Organ => self.synth_organ(freq, sample_rate),
            Instrument::Kick  => self.synth_kick(freq, sample_rate),
            Instrument::Hihat => self.synth_hihat(sample_rate),
            Instrument::Snare => self.synth_snare(freq, sample_rate),
        };
        raw * amplitude
    }

    #[inline]
    fn white_noise(&mut self) -> f32 {
        self.noise_seed = self.noise_seed
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        (self.noise_seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn synth_sine(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        (self.phase * 2.0 * PI).sin()
    }

    fn synth_piano(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        const H: [(f32, f32); 8] = [
            (1.00, 0.8), (0.55, 2.5), (0.28, 5.0), (0.16, 8.5),
            (0.10,13.0), (0.06,18.0), (0.03,25.0), (0.02,34.0),
        ];
        let mut out = 0.0f32;
        for (i, &(amp, rate)) in H.iter().enumerate() {
            out += amp * (-rate * self.t).exp()
                * (self.phase * (i + 1) as f32 * 2.0 * PI).sin();
        }
        (out * 0.7).tanh()
    }

    fn synth_pluck(&mut self) -> f32 {
        if self.ks_len == 0 { return 0.0; }
        let out  = self.ks_buf[self.ks_pos];
        let next = (self.ks_pos + 1) % self.ks_len;
        self.ks_buf[self.ks_pos] = 0.997 * (out * 0.4985 + self.ks_buf[next] * 0.5015);
        self.ks_pos = next;
        out
    }

    fn synth_pad(&mut self, freq: f32, sr: f32) -> f32 {
        const DET: f32 = 0.00173;
        self.phase  = (self.phase  + freq                  / sr) % 1.0;
        self.phase2 = (self.phase2 + freq * (1.0 + DET)    / sr) % 1.0;
        self.phase3 = (self.phase3 + freq * (1.0 - DET)    / sr) % 1.0;
        let s1 = (self.phase  * 2.0 * PI).sin();
        let s2 = (self.phase2 * 2.0 * PI).sin();
        let s3 = (self.phase3 * 2.0 * PI).sin();
        self.flutter_phase = (self.flutter_phase + 0.20 / sr) % 1.0;
        let lfo = 0.82 + 0.18 * (self.flutter_phase * 2.0 * PI).sin();
        ((s1 + s2 * 0.65 + s3 * 0.65) / 2.3) * lfo
    }

    fn synth_bass(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase  = (self.phase  + freq       / sr) % 1.0;
        self.phase2 = (self.phase2 + freq * 0.5 / sr) % 1.0;
        let fund = (self.phase  * 2.0 * PI).sin();
        let sub  = (self.phase2 * 2.0 * PI).sin() * 0.45;
        ((fund + sub) * 1.15).tanh() * 0.82
    }

    fn synth_organ(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        const DB: [(f32, f32); 4] = [(0.5,0.9),(1.0,1.0),(1.5,0.8),(2.0,0.5)];
        let mut out = 0.0f32;
        let mut total = 0.0f32;
        for &(h, amp) in &DB {
            out   += amp * (self.phase * h * 2.0 * PI).sin();
            total += amp;
        }
        (out / total * 1.2).tanh() * 0.85
    }

    fn synth_kick(&mut self, freq: f32, sr: f32) -> f32 {
        let target = freq.max(28.0);
        self.kick_freq += (target - self.kick_freq) * (1.0 - (-400.0 / sr).exp());
        self.phase = (self.phase + self.kick_freq / sr) % 1.0;
        let tone  = (self.phase * 2.0 * PI).sin();
        let click = if self.t < 0.006 {
            self.white_noise() * (1.0 - self.t / 0.006)
        } else { 0.0 };
        // Kick manages its own amplitude decay
        (tone * 0.88 + click * 0.25) * (-9.0 * self.t).exp()
    }

    fn synth_hihat(&mut self, sr: f32) -> f32 {
        let noise = self.white_noise();
        let hp = noise - self.phase + 0.92 * self.phase2;
        self.phase  = noise;
        self.phase2 = hp;
        // Hihat manages its own amplitude decay
        hp * (-32.0 * self.t).exp()
    }

    fn synth_snare(&mut self, freq: f32, sr: f32) -> f32 {
        let body = (freq * 0.8).clamp(180.0, 280.0);
        self.phase = (self.phase + body / sr) % 1.0;
        let tone  = (self.phase * 2.0 * PI).sin() * (-24.0 * self.t).exp();
        let noise = self.white_noise() * (-18.0 * self.t).exp();
        tone * 0.35 + noise * 0.65
    }
}
