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
    pub attack_ms: f32,  // rise time from 0 to peak
    pub decay_ms: f32,   // fall from peak to sustain level
    pub sustain: f32,    // sustain level (0.0-1.0 of peak)
    pub release_ms: f32, // fall from sustain to 0 after note ends
}

impl EnvelopeConfig {
    pub const fn new(attack_ms: f32, decay_ms: f32, sustain: f32, release_ms: f32) -> Self {
        Self {
            attack_ms,
            decay_ms,
            sustain,
            release_ms,
        }
    }
}

// ─────────────────────────────────────────────────────────────
//  INSTRUMENT ENUM + DEFAULT ENVELOPES
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Instrument {
    #[default]
    Sine,
    Piano,
    Pluck,
    Pad,
    Bass,
    Organ,
    Sax,
    Triangle,
    Kick,
    Ride,
    Hihat,
    Snare,
}

impl Instrument {
    // Default ADSR for this instrument.
    // These reflect the natural acoustic behaviour and sound good
    // without any LayerConfig override.
    pub fn default_envelope(self) -> EnvelopeConfig {
        match self {
            // Piano: quick hammer attack, natural decay, gentle release tail.
            Self::Piano => EnvelopeConfig::new(9.0, 820.0, 0.18, 1100.0),

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

            // Sax: slower reed-like attack, rounded sustain, soft release.
            Self::Sax => EnvelopeConfig::new(85.0, 260.0, 0.58, 520.0),

            // Triangle: instant tap with a long, quiet metallic tail.
            Self::Triangle => EnvelopeConfig::new(1.0, 1700.0, 0.0, 2600.0),

            // Sine: smooth attack, no decay, full sustain, medium release.
            Self::Sine => EnvelopeConfig::new(20.0, 0.0, 1.0, 2500.0),

            // Kick: instant attack, fast decay (all in the transient),
            // zero sustain, very short release.
            Self::Kick => EnvelopeConfig::new(1.0, 180.0, 0.0, 30.0),

            // Ride: light stick attack with a longer cymbal wash.
            Self::Ride => EnvelopeConfig::new(1.0, 520.0, 0.0, 90.0),

            // Hihat: instant attack, very fast decay (cymbal shimmer),
            // zero sustain.
            Self::Hihat => EnvelopeConfig::new(1.0, 60.0, 0.0, 20.0),

            // Snare: instant attack, medium decay (body + rattle),
            // zero sustain.
            Self::Snare => EnvelopeConfig::new(1.0, 120.0, 0.0, 25.0),
        }
    }

    pub fn is_percussion(self) -> bool {
        matches!(
            self,
            Self::Kick | Self::Ride | Self::Hihat | Self::Snare | Self::Triangle
        )
    }
}

// ─────────────────────────────────────────────────────────────
//  INSTRUMENT STATE
// ─────────────────────────────────────────────────────────────

pub struct InstrumentState {
    pub phase: f32,
    pub phase2: f32,
    pub phase3: f32,
    pub t: f32,
    noise_seed: u32,
    flutter_phase: f32,
    kick_freq: f32,
    filter_lp: f32,
    hp_prev_in: f32,
    hp_prev_out: f32,
    ks_buf: Vec<f32>,
    ks_pos: usize,
    ks_len: usize,
}

impl InstrumentState {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            phase2: 0.137,
            phase3: 0.271,
            t: 0.0,
            noise_seed: 12345,
            flutter_phase: 0.0,
            kick_freq: 80.0,
            filter_lp: 0.0,
            hp_prev_in: 0.0,
            hp_prev_out: 0.0,
            ks_buf: vec![0.0; 2048],
            ks_pos: 0,
            ks_len: 100,
        }
    }

    pub fn reset(&mut self, freq: f32, sample_rate: f32, instrument: Instrument) {
        self.t = 0.0;
        self.filter_lp = 0.0;
        self.hp_prev_in = 0.0;
        self.hp_prev_out = 0.0;
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
                self.phase = 0.0;
                self.kick_freq = freq * 3.5;
            }
            Instrument::Bass => {
                self.phase = 0.0;
                self.phase2 = 0.231;
                self.phase3 = 0.117;
                let len = (sample_rate / freq.max(35.0)).round() as usize;
                self.ks_len = len.clamp(8, 2047);
                self.ks_pos = 0;
                for i in 0..self.ks_len {
                    let x = i as f32 / self.ks_len as f32;
                    let finger_shape = (x * PI).sin() * 0.18;
                    self.ks_buf[i] = finger_shape + self.white_noise() * 0.11;
                }
            }
            Instrument::Piano => {
                self.phase = 0.0;
                self.phase2 = 0.137;
                self.phase3 = 0.271;
            }
            Instrument::Pad => {
                self.flutter_phase = 0.0;
            }
            Instrument::Sax => {
                self.phase = 0.0;
                self.phase2 = 0.191;
                self.phase3 = 0.337;
                self.flutter_phase = 0.0;
            }
            Instrument::Triangle => {
                self.phase = 0.0;
                self.phase2 = 0.293;
                self.phase3 = 0.617;
            }
            Instrument::Ride | Instrument::Hihat | Instrument::Snare => {
                self.phase = 0.0;
                self.phase2 = 0.0;
                self.phase3 = 0.0;
            }
            _ => {}
        }
    }

    pub fn next_sample(
        &mut self,
        instrument: Instrument,
        freq: f32,
        amplitude: f32,
        sample_rate: f32,
    ) -> f32 {
        self.t += 1.0 / sample_rate;
        let raw = match instrument {
            Instrument::Sine => self.synth_sine(freq, sample_rate),
            Instrument::Piano => self.synth_piano(freq, sample_rate),
            Instrument::Pluck => self.synth_pluck(),
            Instrument::Pad => self.synth_pad(freq, sample_rate),
            Instrument::Bass => self.synth_bass(freq, sample_rate),
            Instrument::Organ => self.synth_organ(freq, sample_rate),
            Instrument::Sax => self.synth_sax(freq, sample_rate),
            Instrument::Triangle => self.synth_triangle(freq, sample_rate),
            Instrument::Kick => self.synth_kick(freq, sample_rate),
            Instrument::Ride => self.synth_ride(sample_rate),
            Instrument::Hihat => self.synth_hihat(),
            Instrument::Snare => self.synth_snare(freq, sample_rate),
        };
        raw * amplitude
    }

    #[inline]
    fn white_noise(&mut self) -> f32 {
        self.noise_seed = self
            .noise_seed
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        (self.noise_seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn low_pass(&mut self, input: f32, cutoff_hz: f32, sr: f32) -> f32 {
        let alpha = (1.0 - (-2.0 * PI * cutoff_hz / sr).exp()).clamp(0.001, 1.0);
        self.filter_lp += (input - self.filter_lp) * alpha;
        self.filter_lp
    }

    fn high_pass(&mut self, input: f32, cutoff_hz: f32, sr: f32) -> f32 {
        let alpha = 1.0 / (1.0 + 2.0 * PI * cutoff_hz / sr);
        let out = alpha * (self.hp_prev_out + input - self.hp_prev_in);
        self.hp_prev_in = input;
        self.hp_prev_out = out;
        out
    }

    fn synth_sine(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        (self.phase * 2.0 * PI).sin()
    }

    fn synth_piano(&mut self, freq: f32, sr: f32) -> f32 {
        let detune = (0.00026 + freq * 0.00000018).clamp(0.00024, 0.00062);
        self.phase = (self.phase + freq / sr) % 1.0;
        self.phase2 = (self.phase2 + freq * (1.0 + detune) / sr) % 1.0;
        self.phase3 = (self.phase3 + freq * (1.0 - detune * 0.72) / sr) % 1.0;

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

        let freq_decay = (freq / 330.0).sqrt().clamp(0.72, 1.45);
        let inharmonicity = (0.000045 + freq * 0.00000008).clamp(0.00004, 0.00016);
        let attack_brightness = (-22.0 * self.t).exp();
        let mut strings = 0.0f32;

        for (i, &(amp, decay_rate)) in PARTIALS.iter().enumerate() {
            let harmonic = (i + 1) as f32;
            let stretch = harmonic * (1.0 + inharmonicity * harmonic * harmonic);
            let decay = (-(decay_rate * freq_decay) * self.t).exp();
            let strike_lift = if i >= 3 {
                1.0 + attack_brightness * 0.46
            } else {
                1.0
            };

            let p1 = (self.phase * stretch * 2.0 * PI).sin();
            let p2 = (self.phase2 * stretch * 2.0 * PI).sin();
            let p3 = (self.phase3 * stretch * 2.0 * PI).sin();
            let unison = p1 * 0.46 + p2 * 0.31 + p3 * 0.23;

            strings += amp * decay * strike_lift * unison;
        }

        let hammer = if self.t < 0.008 {
            let env = (1.0 - self.t / 0.008).powi(2);
            let felt = (self.t * 1180.0 * 2.0 * PI).sin() * 0.004;
            let wood = (self.t * 92.0 * 2.0 * PI).sin() * 0.012;
            (self.white_noise() * 0.006 + felt + wood) * env
        } else {
            0.0
        };

        let key_bed = if self.t < 0.035 {
            (self.t * 48.0 * 2.0 * PI).sin() * (1.0 - self.t / 0.035).powi(2) * 0.008
        } else {
            0.0
        };

        let soundboard = strings * (0.74 + 0.08 * (-2.0 * self.t).exp());
        let raw = (soundboard + hammer + key_bed) * 0.36;
        let note_brightness = ((freq - 180.0) / 720.0).clamp(0.0, 1.0);
        let cutoff = 1450.0 + note_brightness * 650.0 + attack_brightness * 2300.0;
        let warm = self.low_pass(raw, cutoff, sr);
        self.high_pass(warm, 34.0, sr) * 0.86
    }

    fn synth_pluck(&mut self) -> f32 {
        if self.ks_len == 0 {
            return 0.0;
        }
        let out = self.ks_buf[self.ks_pos];
        let next = (self.ks_pos + 1) % self.ks_len;
        self.ks_buf[self.ks_pos] = 0.997 * (out * 0.4985 + self.ks_buf[next] * 0.5015);
        self.ks_pos = next;
        out
    }

    fn synth_pad(&mut self, freq: f32, sr: f32) -> f32 {
        const DET: f32 = 0.00045;
        self.phase = (self.phase + freq / sr) % 1.0;
        self.phase2 = (self.phase2 + freq * (1.0 + DET) / sr) % 1.0;
        self.phase3 = (self.phase3 + freq * 2.0 * (1.0 - DET) / sr) % 1.0;
        let s1 = (self.phase * 2.0 * PI).sin();
        let s2 = (self.phase2 * 2.0 * PI).sin();
        let s3 = (self.phase3 * 2.0 * PI).sin();
        self.flutter_phase = (self.flutter_phase + 0.11 / sr) % 1.0;
        let lfo = 0.92 + 0.06 * (self.flutter_phase * 2.0 * PI).sin();
        let raw = (s1 * 0.55 + s2 * 0.34 + s3 * 0.12) / 1.01;
        let airy = self.high_pass(raw, 150.0, sr);
        self.low_pass(airy, 1800.0, sr) * lfo * 0.58
    }

    fn synth_bass(&mut self, freq: f32, sr: f32) -> f32 {
        let pitch_envelope = 1.0 + 0.006 * (-18.0 * self.t).exp();
        let active_freq = freq * pitch_envelope;

        self.phase = (self.phase + active_freq / sr) % 1.0;
        self.phase2 = (self.phase2 + active_freq * 2.0 / sr) % 1.0;
        self.phase3 = (self.phase3 + active_freq * 0.5 / sr) % 1.0;

        let string = self.next_damped_string_sample(0.9954);
        let pluck_brightness = (-5.0 * self.t).exp();

        let fund = (self.phase * 2.0 * PI).sin() * 0.76;
        let harm = (self.phase2 * 2.0 * PI).sin() * (0.10 + pluck_brightness * 0.035);
        let sub = (self.phase3 * 2.0 * PI).sin() * 0.045;
        let string_mid = string * (0.14 + pluck_brightness * 0.26);

        let finger_click = if self.t < 0.018 {
            let noise = self.white_noise();
            let fingertip = (self.t * 430.0 * 2.0 * PI).sin() * 0.022;
            let nail = (self.t * 860.0 * 2.0 * PI).sin() * 0.006;
            (noise * 0.020 + fingertip + nail) * (1.0 - self.t / 0.018).powi(2)
        } else {
            0.0
        };

        let raw = fund + harm + sub + string_mid + finger_click;
        let shaped = raw / (1.0 + raw.abs() * 0.32);
        let cleaned = self.high_pass(shaped, 30.0, sr);
        self.low_pass(cleaned, 930.0, sr) * 0.58
    }

    fn next_damped_string_sample(&mut self, damping: f32) -> f32 {
        if self.ks_len == 0 {
            return 0.0;
        }

        let out = self.ks_buf[self.ks_pos];
        let next = (self.ks_pos + 1) % self.ks_len;
        let next2 = (self.ks_pos + 2) % self.ks_len;
        self.ks_buf[self.ks_pos] =
            (out * 0.46 + self.ks_buf[next] * 0.46 + self.ks_buf[next2] * 0.08) * damping;
        self.ks_pos = next;
        out
    }

    fn synth_organ(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        const DB: [(f32, f32); 4] = [(0.5, 0.9), (1.0, 1.0), (1.5, 0.8), (2.0, 0.5)];
        let mut out = 0.0f32;
        let mut total = 0.0f32;
        for &(h, amp) in &DB {
            out += amp * (self.phase * h * 2.0 * PI).sin();
            total += amp;
        }
        (out / total * 1.2).tanh() * 0.85
    }

    fn synth_sax(&mut self, freq: f32, sr: f32) -> f32 {
        self.flutter_phase = (self.flutter_phase + 5.0 / sr) % 1.0;
        let vibrato = 1.0 + 0.0045 * (self.flutter_phase * 2.0 * PI).sin();
        let active_freq = freq * vibrato;

        self.phase = (self.phase + active_freq / sr) % 1.0;
        self.phase2 = (self.phase2 + active_freq * 2.0 / sr) % 1.0;
        self.phase3 = (self.phase3 + active_freq * 3.0 / sr) % 1.0;

        let fundamental = (self.phase * 2.0 * PI).sin() * 0.78;
        let second = (self.phase2 * 2.0 * PI).sin() * 0.18;
        let third = (self.phase3 * 2.0 * PI).sin() * 0.07;
        let breath = self.white_noise() * 0.010 * (1.0 - (-14.0 * self.t).exp());

        let shaped = (fundamental + second + third + breath).tanh() * 0.62;
        let mellow = self.low_pass(shaped, 1850.0, sr);
        self.high_pass(mellow, 155.0, sr) * 0.74
    }

    fn synth_triangle(&mut self, freq: f32, sr: f32) -> f32 {
        let strike_freq = freq.clamp(1400.0, 4200.0);
        self.phase = (self.phase + strike_freq / sr) % 1.0;
        self.phase2 = (self.phase2 + strike_freq * 1.505 / sr) % 1.0;
        self.phase3 = (self.phase3 + strike_freq * 2.01 / sr) % 1.0;

        let partials = (self.phase * 2.0 * PI).sin() * 0.56
            + (self.phase2 * 2.0 * PI).sin() * 0.26
            + (self.phase3 * 2.0 * PI).sin() * 0.12;
        let tap = if self.t < 0.006 {
            self.white_noise() * (1.0 - self.t / 0.006).powi(2) * 0.035
        } else {
            0.0
        };

        let fading = (partials + tap) * (-1.15 * self.t).exp();
        let clear = self.high_pass(fading, 950.0, sr);
        self.low_pass(clear, 5200.0, sr) * 0.36
    }

    fn synth_kick(&mut self, freq: f32, sr: f32) -> f32 {
        let target = freq.max(28.0);
        self.kick_freq += (target - self.kick_freq) * (1.0 - (-400.0 / sr).exp());
        self.phase = (self.phase + self.kick_freq / sr) % 1.0;
        let tone = (self.phase * 2.0 * PI).sin();
        let click = if self.t < 0.006 {
            self.white_noise() * (1.0 - self.t / 0.006)
        } else {
            0.0
        };
        // Kick manages its own amplitude decay
        (tone * 0.88 + click * 0.25) * (-9.0 * self.t).exp()
    }

    fn synth_ride(&mut self, sr: f32) -> f32 {
        self.phase = (self.phase + 2140.0 / sr) % 1.0;
        self.phase2 = (self.phase2 + 3180.0 / sr) % 1.0;
        self.phase3 = (self.phase3 + 4720.0 / sr) % 1.0;

        let ring = (self.phase * 2.0 * PI).sin() * 0.34
            + (self.phase2 * 2.0 * PI).sin() * 0.24
            + (self.phase3 * 2.0 * PI).sin() * 0.14;
        let stick = if self.t < 0.008 {
            self.white_noise() * (1.0 - self.t / 0.008).powi(2) * 0.055
        } else {
            0.0
        };
        let wash = self.white_noise() * 0.018 * (-3.2 * self.t).exp();
        let source = (ring + stick + wash) * (-6.2 * self.t).exp();
        let clear = self.high_pass(source, 1150.0, sr);
        self.low_pass(clear, 7200.0, sr) * 0.34
    }

    fn synth_hihat(&mut self) -> f32 {
        let noise = self.white_noise();

        let mut metallic_ring = 0.0;
        let metallic_freqs = [2850.0, 3620.0, 4150.0, 5800.0];
        for &f in &metallic_freqs {
            metallic_ring += (self.t * f * 2.0 * PI).sin();
        }

        let source = noise * 0.65 + (metallic_ring / 4.0) * 0.35;

        let hp = source - self.phase + 0.94 * self.phase2;
        self.phase = source;
        self.phase2 = hp;

        hp * (-36.0 * self.t).exp()
    }

    fn synth_snare(&mut self, freq: f32, sr: f32) -> f32 {
        let body = (freq * 0.8).clamp(180.0, 280.0);
        self.phase = (self.phase + body / sr) % 1.0;
        let tone = (self.phase * 2.0 * PI).sin() * (-24.0 * self.t).exp();
        let noise = self.white_noise() * (-18.0 * self.t).exp();
        tone * 0.35 + noise * 0.65
    }
}
