// Instrument synthesis.
//
// Each instrument is a self-contained state machine.
// InstrumentState::next_sample() advances by one sample and returns f32.
// No allocations, no external dependencies - pure math on the stack.
//
// Adding a new instrument:
//   1. Add variant to Instrument enum
//   2. Add state fields to InstrumentState (unused fields are zero-cost)
//   3. Add synth_* function
//   4. Wire into next_sample() match

use std::f32::consts::PI;

// ─────────────────────────────────────────────────────────────
//  INSTRUMENT ENUM
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Instrument {
    Sine,   // clean sine - neutral electronic
    Piano,  // decaying harmonic stack - acoustic piano feel
    Pluck,  // Karplus-Strong physical model - guitar / harp
    Pad,    // detuned sines + LFO - synth strings
    Bass,   // sine + sub-octave + saturation - bass guitar
    Organ,  // additive harmonics - Hammond organ
    Kick,   // pitch-sweep sine + noise - bass drum
    Hihat,  // filtered white noise - hi-hat
    Snare,  // tone + noise - snare drum
}

impl Instrument {
    pub fn is_percussion(self) -> bool {
        matches!(self, Self::Kick | Self::Hihat | Self::Snare)
    }
}

impl Default for Instrument {
    fn default() -> Self { Self::Sine }
}

// ─────────────────────────────────────────────────────────────
//  INSTRUMENT STATE  (all on the stack, zero heap)
// ─────────────────────────────────────────────────────────────

pub struct InstrumentState {
    pub phase:         f32,
    pub phase2:        f32, // secondary oscillator (detuned layer, sub-octave)
    pub phase3:        f32, // tertiary oscillator  (second detune layer)
    pub t:             f32, // seconds since note-on
    noise_seed:        u32, // LCG random for noise synthesis
    flutter_phase:     f32, // LFO phase for Pad
    kick_freq:         f32, // current sweep frequency for Kick

    // Karplus-Strong delay line (Pluck)
    // 2048 samples covers ~21 Hz at 44100 Hz sample rate
    ks_buf: [f32; 2048],
    ks_pos: usize,
    ks_len: usize,
}

impl InstrumentState {
    pub fn new() -> Self {
        Self {
            phase:  0.0, phase2: 0.137, phase3: 0.271,
            t:      0.0,
            noise_seed:    12345,
            flutter_phase: 0.0,
            kick_freq:     80.0,
            ks_buf: [0.0; 2048],
            ks_pos: 0,
            ks_len: 100,
        }
    }

    // Reset per-note state. Called by Voice::trigger before each new note.
    pub fn reset(&mut self, freq: f32, sample_rate: f32, instrument: Instrument) {
        self.phase  = 0.0;
        self.phase2 = 0.137;
        self.phase3 = 0.271;
        self.t      = 0.0;

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
                // Sweep starts at 3x the note frequency, falls toward ~40 Hz
                self.kick_freq = freq * 3.0;
            }
            _ => {}
        }
    }

    // Advance by one sample and return the output value.
    // `freq` and `amplitude` are already smoothed by the Voice envelope.
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
            Instrument::Pluck => self.synth_pluck(sample_rate),
            Instrument::Pad   => self.synth_pad(freq, sample_rate),
            Instrument::Bass  => self.synth_bass(freq, sample_rate),
            Instrument::Organ => self.synth_organ(freq, sample_rate),
            Instrument::Kick  => self.synth_kick(freq, sample_rate),
            Instrument::Hihat => self.synth_hihat(sample_rate),
            Instrument::Snare => self.synth_snare(freq, sample_rate),
        };

        raw * amplitude
    }

    // ── noise ─────────────────────────────────────────────────

    #[inline]
    fn white_noise(&mut self) -> f32 {
        self.noise_seed = self.noise_seed
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        (self.noise_seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    // ── synthesis functions ───────────────────────────────────

    fn synth_sine(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        (self.phase * 2.0 * PI).sin()
    }

    // Piano: 8 harmonics with individual exponential decays.
    // Higher harmonics decay faster → bright attack, warm sustain.
    fn synth_piano(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        // (amplitude, decay_rate)
        const HARMONICS: [(f32, f32); 8] = [
            (1.00,  1.5), (0.60,  3.5), (0.30,  6.0), (0.18, 10.0),
            (0.10, 14.0), (0.07, 18.0), (0.04, 24.0), (0.02, 32.0),
        ];
        let mut out = 0.0f32;
        for (i, &(amp, decay)) in HARMONICS.iter().enumerate() {
            let env = (-decay * self.t).exp();
            out += amp * env * (self.phase * (i + 1) as f32 * 2.0 * PI).sin();
        }
        out.tanh() // soft clip to prevent harsh peaks
    }

    // Karplus-Strong: noise burst in a low-pass feedback delay line.
    // Delay length = sample_rate / freq → sets pitch.
    fn synth_pluck(&mut self, _sr: f32) -> f32 {
        if self.ks_len == 0 { return 0.0; }
        let out  = self.ks_buf[self.ks_pos];
        let next = (self.ks_pos + 1) % self.ks_len;
        // Low-pass feedback coefficient 0.996 = slow string damping
        self.ks_buf[self.ks_pos] = 0.996 * 0.5 * (out + self.ks_buf[next]);
        self.ks_pos = next;
        out
    }

    // Pad: three slightly detuned oscillators + slow LFO amplitude swell.
    // Beating between oscillators creates the chorus-like pad character.
    fn synth_pad(&mut self, freq: f32, sr: f32) -> f32 {
        let det = 0.003; // 0.3% detune
        self.phase  = (self.phase  + freq             / sr) % 1.0;
        self.phase2 = (self.phase2 + freq * (1.0+det) / sr) % 1.0;
        self.phase3 = (self.phase3 + freq * (1.0-det) / sr) % 1.0;
        let s1 = (self.phase  * 2.0 * PI).sin();
        let s2 = (self.phase2 * 2.0 * PI).sin();
        let s3 = (self.phase3 * 2.0 * PI).sin();
        // LFO at ~0.25 Hz for slow volume swell
        self.flutter_phase = (self.flutter_phase + 0.25 / sr) % 1.0;
        let lfo = 0.85 + 0.15 * (self.flutter_phase * 2.0 * PI).sin();
        ((s1 + s2 * 0.7 + s3 * 0.7) / 2.4) * lfo
    }

    // Bass: fundamental sine + sub-octave + tanh saturation.
    fn synth_bass(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase  = (self.phase  + freq       / sr) % 1.0;
        self.phase2 = (self.phase2 + freq * 0.5 / sr) % 1.0; // sub-octave
        let fund = (self.phase  * 2.0 * PI).sin();
        let sub  = (self.phase2 * 2.0 * PI).sin() * 0.5;
        (fund + sub).tanh() * 0.85
    }

    // Organ: fixed harmonic ratios modelled on Hammond B3 drawbars.
    fn synth_organ(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        // Drawbar amplitudes: 16' 8' 5⅓' 4' (simplified)
        let h = [1.0f32, 1.0, 0.0, 0.5, 0.0];
        let mut out = 0.0f32;
        for (i, &amp) in h.iter().enumerate() {
            if amp > 0.0 {
                out += amp * (self.phase * (i+1) as f32 * 2.0 * PI).sin();
            }
        }
        out / 2.5
    }

    // Kick: exponential frequency sweep + short noise transient.
    fn synth_kick(&mut self, freq: f32, sr: f32) -> f32 {
        let target = freq.max(30.0);
        // Sweep: kick_freq decays toward target
        self.kick_freq += (target - self.kick_freq) * 0.004;
        self.phase = (self.phase + self.kick_freq / sr) % 1.0;
        let tone  = (self.phase * 2.0 * PI).sin();
        // Short click at attack (first 5ms)
        let click = if self.t < 0.005 {
            self.white_noise() * (1.0 - self.t / 0.005)
        } else { 0.0 };
        let env = (-8.0 * self.t).exp();
        (tone * 0.9 + click * 0.3) * env
    }

    // Hihat: high-pass filtered white noise, fast decay.
    fn synth_hihat(&mut self, sr: f32) -> f32 {
        let noise = self.white_noise();
        // One-pole high-pass: y[n] = x[n] - x[n-1] + 0.9*y[n-1]
        let hp = noise - self.phase + 0.90 * self.phase2;
        self.phase  = noise;
        self.phase2 = hp;
        hp * (-28.0 * self.t).exp()
    }

    // Snare: short sine burst + noise, 40/60 mix.
    fn synth_snare(&mut self, freq: f32, sr: f32) -> f32 {
        self.phase = (self.phase + freq / sr) % 1.0;
        let tone  = (self.phase * 2.0 * PI).sin() * (-22.0 * self.t).exp();
        let noise = self.white_noise()            * (-16.0 * self.t).exp();
        tone * 0.4 + noise * 0.6
    }
}
