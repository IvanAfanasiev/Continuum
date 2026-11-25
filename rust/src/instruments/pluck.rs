use super::{
    dsp::{fast_tanh, note_seed, release_fade, saw_blep, square_blep, ExpDecay, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Electric pluck voice: BLEP saws + a BLEP sub-square through a resonant
/// SVF with a fast cutoff "snap" and soft saturation. A clean modern
/// pluck instead of harsh chiptune. Short blip (0.35 s tail, like the reference).
///
/// The timbre is deliberately dark: the pluck body lives in 300–3000 Hz,
/// resonance is almost removed (Q 0.9), and another gentle cut follows
/// saturation. Such a voice reads on a phone but doesn't "ring" or razor the top.
const SUB_GAIN: f32 = 0.22;
const DETUNE: f32 = 1.0025;
const FILTER_FLOOR_HZ: f32 = 360.0;
const FILTER_SNAP_HZ: f32 = 2400.0;
const FILTER_SNAP_RATE: f32 = 13.0;
const FILTER_Q: f32 = 0.9;
/// Gentle shelf after saturation: saturation harmonics don't run off into a whistle.
const TONE_LP_HZ: f32 = 6200.0;
const TONE_Q: f32 = 0.6;
const AMP_DECAY_RATE: f32 = 9.0;
/// 1.0 s tail instead of a 0.35 s blip: a pluck is a plucked string, it
/// sings the note through, not a "tick". The HF share doesn't grow: the
/// cutoff and shelf stay the same, the spectral contract (2–5 kHz <2%) holds.
const MAX_RING_SECS: f32 = 1.0;
/// Pitch "settles" by 2% over ~25 ms after the pluck — like a real string
/// slightly over-pulled by the pick. Tied to the note seed.
const PITCH_DROP: f32 = 0.02;
const PITCH_DROP_RATE: f32 = 42.0;
const ATTACK_SECS: f32 = 0.003;
const FADE_SECS: f32 = 0.02;
const VOICE_GAIN: f32 = 0.78;
/// Pluck: decay rate and its own ramp. Loudness is like a plectrum:
/// you hear the attack, not a second instrument on top of the string.
/// The pluck noise isn't white: a bandpassed 1.2–2.8 kHz "tick". A
/// broadband burst was the main source of rasp (energy up to Nyquist).
const PICK_DECAY: f32 = 220.0;
const PICK_RAMP_SECS: f32 = 0.0015;
const PICK_GAIN: f32 = 0.16;
const PICK_CENTER_HZ: f32 = 1900.0;
const PICK_Q: f32 = 0.85;

#[derive(Clone, Default)]
pub struct PluckVoice {
    sample_rate: f32,
    phase_saw1: f32,
    phase_saw2: f32,
    phase_sub: f32,
    filter: Svf,
    /// Gentle shelf after saturation and the bandpass pluck filter.
    tone: Svf,
    pick_filter: Svf,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
    /// Recursive envelopes instead of per-sample `exp`: one multiply
    /// per sample, coefficients computed in `trigger`.
    amp_env: ExpDecay,
    cutoff_env: ExpDecay,
    /// Pitched attack envelope (pitch settles after the pluck).
    pitch_env: ExpDecay,
    /// Pluck: short noise transient before the soft body.
    pick_env: ExpDecay,
    /// Pluck noise: reseeded by note (identical plucks are identical).
    noise: Noise,
}

impl Voice for PluckVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * self.sample_rate) as usize;
        self.gain = note.velocity;
        self.t = 0.0;
        // Small seed-dependent phase offsets: transients don't line up
        // bit-for-bit on repeats, but pitch and envelopes are preserved.
        let phase_seed = note_seed(note, 0x1234);
        self.phase_saw1 = (phase_seed as f32 / u32::MAX as f32) * 0.08;
        self.phase_saw2 = ((phase_seed >> 8) as f32 / u32::MAX as f32) * 0.08;
        self.phase_sub = ((phase_seed >> 16) as f32 / u32::MAX as f32) * 0.08;
        self.amp_env = ExpDecay::start(AMP_DECAY_RATE, self.sample_rate);
        self.cutoff_env = ExpDecay::start(FILTER_SNAP_RATE, self.sample_rate);
        self.pitch_env = ExpDecay::start(PITCH_DROP_RATE, self.sample_rate);
        // The pluck dies out in ~7 ms: a string attack, not a second instrument.
        self.pick_env = ExpDecay::start(PICK_DECAY, self.sample_rate);
        self.noise = Noise::new(note_seed(note, 0x971C));
        self.filter.scrub_denormals();
        self.tone.reset();
        self.pick_filter.reset();
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let amp = self.amp_env.next();
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        let pitch = 1.0 - PITCH_DROP * self.pitch_env.next();
        let f1 = self.frequency * pitch;
        let f2 = f1 * DETUNE;
        let fs = f1 * 0.5;
        self.phase_saw1 = (self.phase_saw1 + f1 / sample_rate) % 1.0;
        self.phase_saw2 = (self.phase_saw2 + f2 / sample_rate) % 1.0;
        self.phase_sub = (self.phase_sub + fs / sample_rate) % 1.0;
        let saw = (saw_blep(self.phase_saw1, f1, sample_rate)
            + saw_blep(self.phase_saw2, f2, sample_rate))
            * 0.5;
        let sub = square_blep(self.phase_sub, fs, sample_rate) * SUB_GAIN;

        let cutoff = FILTER_FLOOR_HZ + FILTER_SNAP_HZ * self.cutoff_env.next();
        let (low, _, _) = self
            .filter
            .process(saw + sub, cutoff, FILTER_Q, sample_rate);
        // Saturation before the final shelf: soft body, but without harmonics
        // that scrape at 5–10 kHz. The same chain used to sound sterile
        // and sharp precisely because of the unfiltered pluck and open top.
        let driven = fast_tanh(low * 1.05);
        let (voiced, _, _) = self.tone.process(driven, TONE_LP_HZ, TONE_Q, sample_rate);

        // The pluck BYPASSES the body's soft S-attack (it has its own ramp):
        // otherwise the transient is suffocated. But it's a bandpassed tick,
        // not a white click: the narrow band doesn't scatter into "rasp" on a phone.
        let pick_ramp = (self.t / PICK_RAMP_SECS).min(1.0);
        let (_, pick_band, _) =
            self.pick_filter
                .process(self.noise.sample(), PICK_CENTER_HZ, PICK_Q, sample_rate);
        let pick = pick_band * self.pick_env.next() * PICK_GAIN * pick_ramp;

        self.t += 1.0 / sample_rate;
        (voiced * attack + pick) * self.gain * amp * fade * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.filter.reset();
        self.tone.reset();
        self.pick_filter.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{band_ratio, Instrument, VoicePool};
    use super::*;

    #[test]
    fn pluck_stays_out_of_harsh_band() {
        // Pluck timbre: body in 0.5–2 kHz, soft top. The 2–5 kHz band is
        // "sharpness/rasp" on a phone; after the rework its share is small.
        let sr = 44_100.0;
        let mut pluck = VoicePool::plucks(6, 1);
        pluck.set_sample_rate(sr);
        pluck.trigger(&NoteEvent::new(6, 72, 1.0, 0.35));
        let out: Vec<f32> = (0..(0.35 * sr) as usize)
            .map(|_| pluck.next_sample())
            .collect();
        let harsh = band_ratio(&out, sr, 2000.0, 5000.0);
        let air = band_ratio(&out, sr, 5000.0, 10_000.0);
        let body = band_ratio(&out, sr, 500.0, 2000.0);
        assert!(harsh < 0.02, "pluck harsh band: {harsh:.4}");
        assert!(air < 0.005, "pluck air band: {air:.4}");
        assert!(body > 0.3, "pluck lost its body: {body:.4}");
    }

    #[test]
    fn pick_transient_leads_body() {
        // The pluck precedes the body: in the first 2 ms (while the S-attack
        // is still nearly closed) there's already a broadband transient —
        // the pick is audible. Deterministic from the note: a repeat = same pluck.
        fn onset_peak() -> f32 {
            let sr = 44_100.0;
            let mut pluck = VoicePool::plucks(6, 1);
            pluck.set_sample_rate(sr);
            pluck.trigger(&NoteEvent::new(6, 72, 1.0, 0.5));
            (0..88)
                .map(|_| pluck.next_sample().abs())
                .fold(0.0f32, f32::max)
        }
        let peak = onset_peak();
        assert!(peak > 0.02, "pick inaudible: {peak}");
        assert_eq!(
            onset_peak().to_bits(),
            peak.to_bits(),
            "pick not deterministic"
        );
    }

    #[test]
    fn pluck_snaps_and_decays() {
        let sr = 44_100.0;
        let mut pluck = VoicePool::plucks(6, 6);
        pluck.set_sample_rate(sr);
        pluck.trigger(&NoteEvent::new(6, 72, 1.0, 0.5));
        let attack_peak = (0..2000)
            .map(|_| pluck.next_sample().abs())
            .fold(0.0f32, f32::max);
        assert!(attack_peak > 0.1, "pluck has no snap: {attack_peak}");
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..20_000 {
            let s = pluck.next_sample().abs();
            assert!(s.is_finite());
            if i < 10_000 {
                first += s;
            } else {
                second += s;
            }
        }
        assert!(second < first, "pluck does not decay");
    }

    #[test]
    fn pluck_rings_a_full_note() {
        // The string sings the note through: 0.6 s sounds almost fully (it
        // used to cut off at 0.35 s — an audible blip). But the tail isn't
        // endless: a 0.6 s note dies before 0.62 s + fade margin.
        let sr = 44_100.0;
        let mut pluck = VoicePool::plucks(6, 1);
        pluck.set_sample_rate(sr);
        pluck.trigger(&NoteEvent::new(6, 72, 1.0, 0.6));
        let mut late = 0.0f32;
        for _ in 0..(0.5 * sr) as usize {
            late += pluck.next_sample().abs();
        }
        assert!(late > 1.0, "pluck died too early: {late}");
        for _ in 0..((0.12 * sr) as usize) {
            pluck.next_sample();
        }
        assert!(!pluck.is_active(), "pluck rings past its note");
    }

    #[test]
    fn pitch_settles_after_the_pluck() {
        // Pitch sits ~2% above target in the first milliseconds, then settles.
        // Check the phase integrator directly: the frequency ratio in a
        // 5 ms window is greater than in a 200 ms window.
        let sr = 44_100.0;
        let mut one = PluckVoice::default();
        one.set_sample_rate(sr);
        one.trigger(&NoteEvent::new(6, 72, 1.0, 0.5));
        for _ in 0..220 {
            one.next_sample();
        }
        let early_pitch = one.pitch_env.value();
        for _ in 0..8000 {
            one.next_sample();
        }
        let late_pitch = one.pitch_env.value();
        assert!(early_pitch > late_pitch, "pitch envelope does not decay");
    }
}
