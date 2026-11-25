use super::{
    dsp::{fast_tanh, note_seed, release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// 909-style electro kick: body falls 170 → 44 Hz in ~6 ms
/// (a fast sweep = punch lands exactly on the grid instead of drifting),
/// beater click through a band, a short "knock" at 280 Hz.
/// Short tail (0.30 s): a long sub-tail beats against the bass
/// (44 Hz vs ~55 Hz = 11 Hz "frog" ripple).
const CLICK_BP_HZ: f32 = 3500.0;
const CLICK_BP_Q: f32 = 0.8;
const KICK_LEN_SECS: f32 = 0.30;
const FREQ_START_HZ: f32 = 170.0;
const FREQ_END_HZ: f32 = 44.0;
const SWEEP_RATE: f32 = 160.0;
const AMP_DECAY: f32 = 7.5;
const CLICK_DECAY: f32 = 300.0;
/// Click louder (was 0.6): on small speakers the fundamental
/// is inaudible, the beat rides on the beater. Short tail — no mud.
const CLICK_GAIN: f32 = 0.7;
const KNOCK_HZ: f32 = 280.0;
const KNOCK_DECAY: f32 = 55.0;
const KNOCK_GAIN: f32 = 0.3;
const DRIVE: f32 = 1.1;
/// No more grit offset (was 0.1): the DC skew flattened the wave
/// in tanh on every hit (the rasp of a solo kick) — a clean hit
/// keeps its punch from the body sweep, not from overload. Drive lowered from 1.25:
/// the transient must cover the bass sustain, flat-topping does not.
const ATTACK_SECS: f32 = 0.001;
const FADE_SECS: f32 = 0.012;
/// Kick output. The sum of body, "knock" and beater reached 1.09 — and with
/// the retrigger tail (previous hit still sounding) over 1.09+, clipping:
/// every other hit had a click. Normalize so the hit and its
/// overlap stay within unity.
const OUT_GAIN: f32 = 0.78;

#[derive(Clone, Default)]
struct KickVoice {
    sample_rate: f32,
    t: f32,
    phase: f32,
    knock_phase: f32,
    gain: f32,
    samples_left: usize,
    noise: Noise,
    click_bp: Svf,
    /// Recursive envelopes instead of per-sample `exp`.
    body_env: ExpDecay,
    sweep_env: ExpDecay,
    knock_env: ExpDecay,
    click_env: ExpDecay,
}

impl Voice for KickVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.samples_left = (KICK_LEN_SECS * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        self.phase = 0.0;
        self.knock_phase = 0.0;
        self.body_env = ExpDecay::start(AMP_DECAY, self.sample_rate);
        self.sweep_env = ExpDecay::start(SWEEP_RATE, self.sample_rate);
        self.knock_env = ExpDecay::start(KNOCK_DECAY, self.sample_rate);
        self.click_env = ExpDecay::start(CLICK_DECAY, self.sample_rate);
        self.click_bp.reset();
        // Click noise reseeded from the note: identical hits are identical,
        // differing velocity gives live variation instead of machine-gun.
        self.noise = Noise::new(note_seed(note, 0xC10C));
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let body_env = self.body_env.next();
        let freq = FREQ_END_HZ + (FREQ_START_HZ - FREQ_END_HZ) * self.sweep_env.next();
        self.phase = (self.phase + freq / sample_rate) % 1.0;
        let body = sin01(self.phase);

        self.knock_phase = (self.knock_phase + KNOCK_HZ / sample_rate) % 1.0;
        let knock = sin01(self.knock_phase) * self.knock_env.next() * KNOCK_GAIN;

        // The beater click lives the first ~5 ms: noise through a 3.5 kHz band —
        // a mallet strike, not raw white noise.
        let raw_click = self.noise.sample() * self.click_env.next();
        let (_, click_band, _) =
            self.click_bp
                .process(raw_click, CLICK_BP_HZ, CLICK_BP_Q, sample_rate);
        let click = click_band * CLICK_GAIN;

        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        // S-curve attack: linear leaves corners (micro-clicks on frequent hits).
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        fast_tanh((body + knock + click) * DRIVE) * self.gain * body_env * attack * fade * OUT_GAIN
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.phase = 0.0;
        self.gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Public instrument: monophonic voice with tail crossfade.
pub struct Kick(Mono<KickVoice>);

impl Kick {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Kick))
    }
}

impl Instrument for Kick {
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
    fn kick_rings_finite_and_loud() {
        let sr = 44_100.0;
        let mut kick = Kick::new(2);
        kick.set_sample_rate(sr);
        kick.trigger(&NoteEvent::new(2, 36, 1.0, 0.1));
        let mut peak: f32 = 0.0;
        for _ in 0..((KICK_LEN_SECS * sr) as usize) {
            let s = kick.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.3, "kick too quiet: {peak}");
        assert!(!kick.is_active());
    }

    #[test]
    fn retrigger_has_no_audible_step() {
        // Click regression on frequent repeats: retriggering into a loud tail
        // must not produce a step — the tail fades via crossfade.
        let sr = 44_100.0;
        let mut kick = Kick::new(2);
        kick.set_sample_rate(sr);
        kick.trigger(&NoteEvent::new(2, 36, 1.0, 0.4));
        for _ in 0..2000 {
            kick.next_sample();
        }
        kick.trigger(&NoteEvent::new(2, 36, 1.0, 0.4));
        let mut prev = kick.next_sample();
        assert!(prev.is_finite());
        let mut max_step: f32 = 0.0;
        for _ in 0..500 {
            let s = kick.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
        }
        assert!(max_step < 0.5, "retrigger step: {max_step}");
    }
}
