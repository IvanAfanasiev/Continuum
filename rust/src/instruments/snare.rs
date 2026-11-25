use super::{
    dsp::{fast_tanh, note_seed, release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// 808-style electro snare: tonal body + noise through a resonant SVF band
/// (instead of a crude HP+LP stack — tighter punch, less hiss)
/// + a "snap" in the first millisecond.
///
/// Two characters by note pitch: low (MIDI below `CLAP_PITCH`) —
/// a drum with a body, high — a 909 clap (almost no body, noise
/// brighter and shorter). Pitch used to be ignored, and the club "clap"
/// sounded like a low drum: 67% of energy below 120 Hz.
const SNARE_LEN_SECS: f32 = 0.22;
/// Threshold for "this is a clap, not a drum" (MIDI).
const CLAP_PITCH: u8 = 55;
/// Three body modes instead of one sine: 180/330/450 Hz with DIFFERENT
/// decays. One tone sounded like a "barrel"; three give a taut membrane
/// with an inharmonic ring — like a real 909.
const BODY_MODES_HZ: [f32; 3] = [180.0, 330.0, 450.0];
const BODY_MODES_DECAY: [f32; 3] = [38.0, 62.0, 88.0];
const BODY_MODES_GAIN: [f32; 3] = [0.62, 0.34, 0.2];
const BODY_SWEEP: f32 = 30.0;
/// Overall body envelope (on top of the modes).
const BODY_DECAY: f32 = 38.0;
/// 909 clap: three bursts ~10 ms apart with a shared tail.
const CLAP_BURST_GAP_SECS: f32 = 0.010;
const CLAP_BURST_DECAY: f32 = 260.0;
const CLAP_TAIL_START_SECS: f32 = 0.030;
const CLAP_TAIL_DECAY: f32 = 28.0;
const NOISE_CENTER_HZ: f32 = 2200.0;
/// Clap noise center: higher and brighter than the drum's.
const CLAP_NOISE_CENTER_HZ: f32 = 3000.0;
const NOISE_Q: f32 = 0.9;
const NOISE_DECAY: f32 = 22.0;
const NOISE_GAIN: f32 = 1.0;
const SNAP_DECAY: f32 = 300.0;
/// Snap — a short bright attack burst. It used to be RAW white
/// noise: band from 0 Hz, and a micro-crackle on every hit. Now only
/// the top (4 kHz and up) — closer to the 909, where the "chk" is narrow.
const SNAP_HP_HZ: f32 = 4000.0;
const SNAP_GAIN: f32 = 0.45;
const ATTACK_SECS: f32 = 0.0005;
const FADE_SECS: f32 = 0.008;

#[derive(Clone, Default)]
struct SnareVoice {
    sample_rate: f32,
    t: f32,
    /// Phases and envelopes of the three body modes.
    phases: [f32; 3],
    mode_envs: [ExpDecay; 3],
    gain: f32,
    samples_left: usize,
    noise: Noise,
    bp: Svf,
    /// Separate highpass for the snap: the attack burst must be narrow.
    snap_hp: Svf,
    /// Recursive envelopes instead of per-sample `exp`.
    sweep_env: ExpDecay,
    body_env: ExpDecay,
    snap_env: ExpDecay,
    hiss_env: ExpDecay,
    /// Clap: current burst 0..3 (3 = tail).
    clap_stage: u8,
    clap_env: ExpDecay,
    /// Clap (high note) or drum: different body and noise center.
    clap: bool,
}

impl Voice for SnareVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.samples_left = (SNARE_LEN_SECS * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        for (phase, hz) in self.phases.iter_mut().zip(BODY_MODES_HZ) {
            *phase = (hz * 0.001) % 1.0; // deterministic phase spread
        }
        for (env, decay) in self.mode_envs.iter_mut().zip(BODY_MODES_DECAY) {
            *env = ExpDecay::start(decay, self.sample_rate);
        }
        self.sweep_env = ExpDecay::start(BODY_SWEEP, self.sample_rate);
        self.body_env = ExpDecay::start(BODY_DECAY, self.sample_rate);
        self.snap_env = ExpDecay::start(SNAP_DECAY, self.sample_rate);
        self.hiss_env = ExpDecay::start(NOISE_DECAY, self.sample_rate);
        self.clap_stage = 0;
        self.clap_env = ExpDecay::start(CLAP_BURST_DECAY, self.sample_rate);
        self.bp.reset();
        self.snap_hp.reset();
        // Clap or drum: decided by note pitch (the style sets the
        // instrument's character, not just the timbre).
        self.clap = note.midi_pitch >= CLAP_PITCH;
        self.noise = Noise::new(note_seed(note, 0x5A4E));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        // Shared descending mode sweep (like a taut membrane) + three
        // independent decays: high modes die before the low ones.
        let sweep = 1.0 - 0.12 * self.sweep_env.next();
        let mut body = 0.0f32;
        for i in 0..3 {
            let freq = BODY_MODES_HZ[i] * sweep;
            let phase = &mut self.phases[i];
            *phase = (*phase + freq / sample_rate) % 1.0;
            body += (sin01(*phase) + 0.18 * sin01(*phase * 2.0))
                * BODY_MODES_GAIN[i]
                * self.mode_envs[i].next();
        }
        // The clap has almost no body: a clap is noise, not a drum.
        let body_gain = if self.clap { 0.12 } else { 0.7 };
        let body = body * self.body_env.next() * body_gain;

        // 909 clap: three bursts 10 ms apart + a long tail from 30 ms.
        // Each burst is reseeded by its own ExpDecay — no allocations.
        if self.clap {
            if self.clap_stage < 3 && self.t >= (self.clap_stage as f32 + 1.0) * CLAP_BURST_GAP_SECS
            {
                self.clap_stage += 1;
                self.clap_env = ExpDecay::start(CLAP_BURST_DECAY, sample_rate);
            }
            if self.clap_stage == 3 && self.t >= CLAP_TAIL_START_SECS {
                self.clap_stage = 4;
                self.clap_env = ExpDecay::start(CLAP_TAIL_DECAY, sample_rate);
            }
        }

        let raw_noise = self.noise.sample();
        let snap = raw_noise * self.snap_env.next();
        // Snap — top only: a narrow attack burst instead of white crackle.
        let (_, _, snap_bright) = self.snap_hp.process(snap, SNAP_HP_HZ, NOISE_Q, sample_rate);
        let snap = snap_bright * SNAP_GAIN;
        let center = if self.clap {
            CLAP_NOISE_CENTER_HZ
        } else {
            NOISE_CENTER_HZ
        };
        let (_, hiss, _) = self.bp.process(raw_noise, center, NOISE_Q, sample_rate);
        // For the clap, noise is controlled by the three-burst structure; for
        // the drum, by a normal exponential envelope.
        let noise_env = if self.clap {
            self.clap_env.next()
        } else {
            self.hiss_env.next()
        };
        let hiss = hiss * noise_env * NOISE_GAIN;

        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        fast_tanh((body + hiss + snap) * 1.4) * 0.85 * self.gain * attack * fade
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Public instrument: monophonic voice with tail crossfade.
pub struct SnareDrum(Mono<SnareVoice>);

impl SnareDrum {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Snare))
    }
}

impl Instrument for SnareDrum {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.0.set_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.0.trigger(note);
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
    }

    fn is_active(&self) -> bool {
        self.0.is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snare_rings_finite_and_loud() {
        let sr = 44_100.0;
        let mut snare = SnareDrum::new(4);
        snare.set_sample_rate(sr);
        snare.trigger(&NoteEvent::new(4, 38, 1.0, 0.1));
        let mut peak: f32 = 0.0;
        for _ in 0..((SNARE_LEN_SECS * sr) as usize) {
            let s = snare.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.3, "snare too quiet: {peak}");
        assert!(!snare.is_active());
    }

    #[test]
    fn clap_has_three_bursts_and_a_tail() {
        // 909 clap: three bursts 10 ms apart and a tail after 30 ms.
        // With just one band of noise there would be no dips between
        // bursts — this test catches it.
        let sr = 44_100.0;
        let mut snare = SnareDrum::new(4);
        snare.set_sample_rate(sr);
        snare.trigger(&NoteEvent::new(4, 60, 1.0, 0.1)); // 60 ≥ CLAP_PITCH
        let out: Vec<f32> = (0..(0.12 * sr) as usize)
            .map(|_| snare.next_sample())
            .collect();
        let rms = |from_ms: f32, to_ms: f32| {
            let a = (from_ms / 1000.0 * sr) as usize;
            let b = (to_ms / 1000.0 * sr) as usize;
            (out[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt()
        };
        let b1 = rms(0.0, 5.0);
        let valley1 = rms(6.0, 9.0);
        let b2 = rms(10.0, 15.0);
        let valley2 = rms(16.0, 19.0);
        let b3 = rms(20.0, 25.0);
        let tail = rms(35.0, 70.0);
        assert!(b1 > valley1 * 1.3, "no first clap burst: {b1} vs {valley1}");
        assert!(b2 > valley1 * 1.3, "no second clap burst: {b2}");
        assert!(b3 > valley2 * 1.3, "no third clap burst: {b3} vs {valley2}");
        assert!(tail > 1e-4, "clap tail missing: {tail}");
    }
}
