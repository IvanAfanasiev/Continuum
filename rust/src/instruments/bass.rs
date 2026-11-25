use super::{
    dsp::{fast_tanh, release_fade, saw_blep, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Deep sub-bass: one sine oscillator, two paths.
/// A pure sub gives depth (a sine can't buzz or alias), gentle saturation
/// of the same wave gives warm presence on small speakers
/// (the brain reconstructs the fundamental from harmonics).
/// The even octave (+12 semitones, ~0.3 of the fundamental) is tube
/// warmth as in the references: odd tanh alone gives only a "hollow"
/// square timbre. No detuned saws or resonance: those produced the
/// "keyboard" rasp in the previous version.
const SUB_GAIN: f32 = 0.9;
const OCTAVE_GAIN: f32 = 0.5;
/// The bass's second harmonic: holds the low end on phones, where a pure
/// sine disappears. Independent of the style's sub bias — it's an "iron"
/// layer, not a color tint.
const SECOND_HARMONIC_GAIN: f32 = 0.26;
const PRESENCE_GAIN: f32 = 0.45;
/// A small saw addition for presence: 15% of the layer. Gives upper harmonics
/// on small speakers without turning the bass into a buzzing synth. The layer
/// goes through the same keytracked filter, so high notes don't buzz.
const SAW_PRESENCE_GAIN: f32 = 0.15;
/// Wave drive. It used to be 1.5 + 2.5·velocity (up to 3.75): such overload
/// gave an almost square timbre — a coarse "rasp" at the very bottom.
/// Now it's gentle saturation: warmth remains, no grinding.
const DRIVE_BASE: f32 = 1.2;
const DRIVE_VEL: f32 = 1.3;
/// Output normalization. The sum of bass layers (sub + octave + presence + pick)
/// reached 1.7, and the bass clipped by itself in the engine: peak 1.35 with
/// 2675 clipped samples — that is the rasp in the low end. Normalize so that
/// one note and its overlap with the tail stay within unity.
const OUT_GAIN: f32 = 0.38;
const PRESENCE_Q: f32 = 0.7;
/// Pick transient: a short noise burst. It's audible in the reference's
/// picked bass; without it the attack is faceless.
const PICK_DECAY: f32 = 120.0;
const PICK_GAIN: f32 = 0.12;
const PITCH_DROP: f32 = 0.025;
const PITCH_DROP_RATE: f32 = 80.0;
/// Attack with a fade instead of a click: 10 ms + seed softness on top.
/// Long release (35 ms): the bass sings through, it doesn't smack.
const ATTACK_SECS: f32 = 0.010;
const AMP_DECAY_RATE: f32 = 8.0;
const SUSTAIN_LEVEL: f32 = 0.85;
const FADE_SECS: f32 = 0.035;
/// Its own filter envelope: 8 ms attack → peak, then a fall to half at 300 Hz
/// over ~300 ms. The cutoff used to simply drop from 680 Hz from the first
/// sample, and the attack was "open" without a real pick. Now the filter
/// breathes independently of the amplitude envelope.
const FILTER_ATTACK_SECS: f32 = 0.008;
const FILTER_PEAK_HZ: f32 = 680.0;
const FILTER_FLOOR_HZ: f32 = 300.0;
const FILTER_DECAY_SECS: f32 = 0.3;
/// Cutoff keytracking: higher note — higher filter peak. Bass stays deep,
/// but presence doesn't drop out in the upper register of a bassline.
const KEYTRACK_REF_HZ: f32 = 55.0;

#[derive(Clone, Default)]
struct BassVoice {
    sample_rate: f32,
    phase: f32,
    frequency: f32,
    gain: f32,
    velocity: f32,
    elapsed: usize,
    samples_left: usize,
    presence_lp: Svf,
    presence_lp2: Svf,
    attack_mul: f32,
    release_mul: f32,
    sub_bias: f32,
    noise: Noise,
    /// Live pitch and loudness: legato glides to the target (~30/5 ms)
    /// rather than jumping — a walking bassline flows instead of being chopped.
    freq_now: f32,
    gain_target: f32,
    /// Recursive envelopes and glide coefficients: computed in
    /// `trigger`/`set_sample_rate`, then not a single `exp`/`sin`.
    amp_env: ExpDecay,
    drop_env: ExpDecay,
    cutoff_env: ExpDecay,
    pick_env: ExpDecay,
    glide_k: f32,
    gain_k: f32,
}

impl BassVoice {
    fn update_rate_ks(&mut self) {
        let sr = self.sample_rate.max(1.0);
        self.glide_k = 1.0 - (-1.0 / (sr * 0.03)).exp();
        self.gain_k = 1.0 - (-1.0 / (sr * 0.005)).exp();
    }
}

impl Voice for BassVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
        self.update_rate_ks();
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        self.freq_now = self.frequency;
        self.samples_left = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.gain = note.velocity;
        self.gain_target = note.velocity;
        self.velocity = note.velocity;
        // Small seed-dependent phase offset: the bass doesn't start bit-for-bit
        // identically, but pitch and envelope are preserved.
        self.phase = (super::dsp::note_seed(note, 0x8A55) as f32 / u32::MAX as f32) * 0.05;
        self.elapsed = 0;
        self.attack_mul = note.attack_mul;
        self.release_mul = note.release_mul;
        self.sub_bias = note.sub_bias.clamp(0.0, 1.0);
        self.noise = Noise::new(super::dsp::note_seed(note, 0x8A55));
        self.amp_env = ExpDecay::start(AMP_DECAY_RATE, self.sample_rate);
        self.drop_env = ExpDecay::start(PITCH_DROP_RATE, self.sample_rate);
        self.cutoff_env = ExpDecay::start(1.0 / FILTER_DECAY_SECS, self.sample_rate);
        self.pick_env = ExpDecay::start(PICK_DECAY, self.sample_rate);
        self.update_rate_ks();
        self.presence_lp.reset();
        self.presence_lp2.reset();
    }

    fn has_legato(&self) -> bool {
        true
    }

    fn legato(&mut self, note: &NoteEvent) {
        // New pitch without a restart: phase and envelopes keep flowing,
        // the pick isn't reseeded (it has already rung out), filters don't break.
        // Only the glide target, extension and fresh seed articulation change.
        self.frequency = midi_to_freq(note.midi_pitch);
        let want = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.samples_left = self.samples_left.max(want);
        self.gain_target = note.velocity;
        self.velocity = note.velocity;
        self.attack_mul = note.attack_mul;
        self.release_mul = note.release_mul;
        self.sub_bias = note.sub_bias.clamp(0.0, 1.0);
    }

    fn extend(&mut self, note: &NoteEvent) {
        // A tie is a continuation of the same note: don't touch phase, attack
        // or timbre — only extend the tail and smoothly move dynamics to the new one.
        if self.samples_left == 0 {
            self.trigger(note);
            return;
        }
        let want = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.samples_left = self.samples_left.max(want);
        self.gain_target = note.velocity;
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let t = self.elapsed as f32 / sample_rate;
        self.elapsed += 1;

        // Legato glide: pitch moves to the target in ~30 ms, loudness in ~5 ms.
        // The phase doesn't break (same integrator), the attack doesn't restart.
        // Coefficients are precomputed (see `trigger`).
        self.freq_now += (self.frequency - self.freq_now) * self.glide_k;
        self.gain += (self.gain_target - self.gain) * self.gain_k;

        // Soft S-attack instead of linear: no click, no mush.
        // Attack length comes from the seed: some seeds are punchier, others softer.
        let attack_secs = ATTACK_SECS * self.attack_mul.max(0.25);
        let a = (t / attack_secs).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let amp = SUSTAIN_LEVEL + (1.0 - SUSTAIN_LEVEL) * self.amp_env.next();
        let fade_len = (FADE_SECS * self.release_mul.max(0.25) * sample_rate) as usize;
        let fade = release_fade(self.samples_left, fade_len);

        let freq =
            self.freq_now * (1.0 - PITCH_DROP * (1.0 - 0.7 * self.sub_bias) * self.drop_env.next());
        self.phase = (self.phase + freq / sample_rate) % 1.0;
        let wave = sin01(self.phase);
        // The even octave is exactly in phase with the fundamental (same phase ×2):
        // warmth without beating — harmonics coincide instead of arguing.
        let octave = sin01(self.phase * 2.0) * OCTAVE_GAIN;
        // The second harmonic is a separate layer, NOT dependent on the style's
        // sub bias. A pure sine is inaudible on a phone: the low end drops out and
        // the bass vanishes entirely. An 808 is built exactly this way — fundamental
        // plus second harmonic; that's what holds the bass on a phone.
        // PHASE 0: there used to be a 0.15 offset here, which made the second
        // harmonic beat against the octave layer (interference → "rasp"
        // and volume dips every period). The layers are coherent.
        let second = sin01(self.phase * 2.0) * SECOND_HARMONIC_GAIN;

        let sub = wave * SUB_GAIN;
        // The same wave through drive: harmonics are perfectly coherent with
        // the fundamental — warmth without beating or chorus.
        // The style's sub bias tames the presence chain (the "string" edge) in
        // favor of a pure sine: club is deep and even, rock is stringy (0.0).
        // Cutoff envelope (deep house): tanh harmonics dive 680→300 Hz
        // over ~300 ms (a pick in the attack), while the even octave BYPASSES
        // the filter — tube warmth stays steady throughout the sustain.
        let drive = DRIVE_BASE + DRIVE_VEL * self.velocity;
        // Its own filter attack: 8 ms up, then a ~300 ms fall.
        // Keytracking raises the cutoff for high bass notes.
        let fa = (t / FILTER_ATTACK_SECS).min(1.0);
        let filter_attack = fa * fa * (3.0 - 2.0 * fa);
        let key = (freq / KEYTRACK_REF_HZ).clamp(0.75, 2.2);
        let cutoff = (FILTER_FLOOR_HZ
            + (FILTER_PEAK_HZ - FILTER_FLOOR_HZ) * filter_attack * self.cutoff_env.next())
            * key;
        // The saw goes through the same filter as the tanh layer: presence
        // appears only in the attack and leaves with the cutoff.
        let src = fast_tanh(wave * drive)
            + saw_blep(self.phase, freq, sample_rate) * SAW_PRESENCE_GAIN;
        let (presence, _, _) =
            self.presence_lp
                .process(src, cutoff, PRESENCE_Q, sample_rate);
        // Second cascade (24 dB/oct total): a single one let tanh harmonics
        // through far up — the dive wasn't audible.
        let (presence, _, _) = self
            .presence_lp2
            .process(presence, cutoff, PRESENCE_Q, sample_rate);

        let keep = 1.0 - 0.85 * self.sub_bias;
        // Pick: a broadband transient only in the attack (8 ms).
        // With sub bias it's quieter but doesn't vanish: a picked sub picks too.
        // Its own fast ramp (2 ms): the click comes before the soft body,
        // otherwise the 10 ms attack suffocates it.
        let pa = (t / 0.002).min(1.0);
        let pick = self.noise.sample()
            * self.pick_env.next()
            * PICK_GAIN
            * (1.0 - 0.5 * self.sub_bias)
            * pa;
        (sub + second + (presence * PRESENCE_GAIN + octave * OCTAVE_GAIN) * keep + pick)
            * self.gain
            * amp
            * attack
            * fade
            * OUT_GAIN
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.elapsed = 0;
        self.gain = 0.0;
        self.presence_lp.reset();
        self.presence_lp2.reset();
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Public instrument: monophonic voice with tail crossfade.
/// Retriggering on 16th-note drops doesn't chop the sustain with a step.
pub struct BassGuitar(Mono<BassVoice>);

impl BassGuitar {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Bass))
    }
}

impl Instrument for BassGuitar {
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

    fn goertzel(samples: &[f32], target_hz: f32, sample_rate: f32) -> f32 {
        let n = samples.len() as f32;
        let k = (target_hz / sample_rate * n).round();
        let omega = 2.0 * std::f32::consts::PI * k / n;
        let coeff = 2.0 * omega.cos();
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in samples {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt()
    }

    fn render_bass(pitch: u8, vel: f32, secs: f32) -> Vec<f32> {
        let sr = 44_100.0;
        let mut bass = BassGuitar::new(1);
        bass.set_sample_rate(sr);
        bass.trigger(&NoteEvent::new(1, pitch, vel, secs));
        (0..(secs * sr) as usize)
            .map(|_| bass.next_sample())
            .collect()
    }

    #[test]
    fn filter_dives_from_pick_to_sub() {
        // Cutoff envelope: brighter in the first 100 ms (pick), darker at 400–500 ms.
        // Measure the energy of the first difference (HF) in the time domain —
        // without leakage from the rectangular window of Goertzel.
        fn hf_share(from_ms: f32, to_ms: f32) -> f32 {
            let sr = 44_100.0;
            let out = render_bass(40, 1.0, 1.0);
            let seg = &out[(from_ms / 1000.0 * sr) as usize..(to_ms / 1000.0 * sr) as usize];
            let mut hf = 0.0;
            let mut tot = 0.0;
            let mut prev = 0.0;
            for &s in seg {
                hf += (s - prev) * (s - prev);
                tot += s * s;
                prev = s;
            }
            hf / tot.max(1e-9)
        }
        assert!(
            hf_share(15.0, 100.0) > hf_share(400.0, 500.0),
            "filter does not dive: early={} late={}",
            hf_share(15.0, 100.0),
            hf_share(400.0, 500.0)
        );
    }

    #[test]
    fn long_note_sustains_beyond_old_decay() {
        let sr = 44_100.0;
        let out = render_bass(40, 1.0, 1.0);
        let mut sum_sq = 0.0;
        let mut n = 0;
        for (i, &s) in out.iter().enumerate() {
            assert!(s.is_finite());
            if i >= (0.5 * sr) as usize {
                sum_sq += s * s;
                n += 1;
            }
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms > 0.05, "bass does not sustain: rms={rms}");
    }

    #[test]
    fn low_c2_root_is_loud() {
        // Regression "no bass in the bass": the C2 root (65 Hz) must give
        // substantial sustain, not a whisper.
        let sr = 44_100.0;
        let out = render_bass(36, 1.0, 1.0);
        let mut sum_sq = 0.0;
        let mut n = 0;
        for (i, &s) in out.iter().enumerate() {
            assert!(s.is_finite());
            if i >= (0.3 * sr) as usize {
                sum_sq += s * s;
                n += 1;
            }
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms > 0.08, "no bass weight: rms={rms}");
    }

    #[test]
    fn sub_fundamental_dominates_spectrum() {
        // A2 is exactly 110 Hz (a whole-bin Goertzel bin at 1 Hz): the
        // fundamental's energy must exceed the upper mids by a multiple.
        let sr = 44_100.0;
        let out = render_bass(45, 1.0, 1.2);
        let sub = goertzel(&out, 110.0, sr);
        let upper = goertzel(&out, 1000.0, sr);
        assert!(
            sub > upper * 3.0,
            "weak fundamental: sub={sub} upper={upper}"
        );
    }

    #[test]
    fn sub_bias_pushes_bass_toward_pure_sine() {
        // A bias of 1.0 tames the presence chain (drive harmonics): the
        // second harmonic weakens. But the baseline second harmonic remains —
        // on a phone a pure sine disappears entirely and the bass is inaudible.
        fn second_ratio(bias: f32) -> (f32, f32) {
            let sr = 44_100.0;
            let mut bass = BassGuitar::new(1);
            bass.set_sample_rate(sr);
            let mut note = NoteEvent::new(1, 45, 1.0, 1.0);
            note.sub_bias = bias;
            bass.trigger(&note);
            let out: Vec<f32> = (0..sr as usize).map(|_| bass.next_sample()).collect();
            assert!(out.iter().all(|s| s.is_finite()));
            (goertzel(&out, 110.0, sr), goertzel(&out, 220.0, sr))
        }
        let (fund0, second0) = second_ratio(0.0);
        let (fund1, second1) = second_ratio(1.0);
        assert!(second0 > fund0 * 0.15, "baseline warmth lost");
        assert!(
            second1 < second0 * 0.7,
            "presence didn't die out: {second0} -> {second1}"
        );
        assert!(
            second1 > fund1 * 0.12,
            "bass became a pure sine: 2nd={second1} fund={fund1}"
        );
        assert!(
            fund1 > fund0 * 0.5,
            "fundamental collapsed: {fund1} vs {fund0}"
        );
    }

    #[test]
    fn even_harmonic_adds_tube_warmth() {
        // Reference (Bad Angel, C2 65 Hz): 2nd harmonic ~0.3 of the fundamental.
        // A pure sine + tanh give only odd ones — hence the "flatness".
        // Measured at A2 (110 Hz, whole bin): 2nd is exactly 220 Hz.
        let sr = 44_100.0;
        let out = render_bass(45, 1.0, 1.0);
        let fund = goertzel(&out, 110.0, sr);
        let second = goertzel(&out, 220.0, sr);
        assert!(
            second > fund * 0.15,
            "no even warmth: 2nd={second} fund={fund}"
        );
        assert!(second < fund * 0.45, "too honky: 2nd={second} fund={fund}");
    }

    #[test]
    fn no_buzzy_harmonics() {
        // A2 is exactly 110 Hz: the third harmonic (330 Hz, also a whole bin)
        // must be moderate. For a saw it's 0.33 of the fundamental —
        // that's the buzz; for gentle saturation it's many times less.
        let sr = 44_100.0;
        let out = render_bass(45, 1.0, 1.0);
        let fund = goertzel(&out, 110.0, sr);
        let third = goertzel(&out, 330.0, sr);
        assert!(third < fund * 0.45, "buzzy bass: fund={fund} 3rd={third}");
    }

    #[test]
    fn short_note_is_short() {
        let sr = 44_100.0;
        let mut bass = BassGuitar::new(1);
        bass.set_sample_rate(sr);
        bass.trigger(&NoteEvent::new(1, 40, 1.0, 0.1));
        for _ in 0..((0.1 * sr) as usize + 10) {
            bass.next_sample();
        }
        assert!(!bass.is_active());
    }

    #[test]
    fn seed_articulation_softens_attack() {
        // A soft seed (long attack) gives less energy in the first 20 ms
        // than a sharp one — articulation actually works, not ignored.
        fn early_energy(atk: f32, rel: f32) -> f32 {
            let sr = 44_100.0;
            let mut bass = BassGuitar::new(1);
            bass.set_sample_rate(sr);
            bass.trigger(&NoteEvent::new(1, 40, 0.9, 0.5).shaped(atk, rel));
            (0..(0.02 * sr) as usize)
                .map(|_| {
                    let s = bass.next_sample();
                    assert!(s.is_finite());
                    s.abs()
                })
                .sum()
        }
        let sharp = early_energy(0.8, 0.8);
        let soft = early_energy(1.8, 1.6);
        assert!(
            soft < sharp,
            "articulation ignored: soft={soft} sharp={sharp}"
        );
    }

    #[test]
    fn legato_glides_without_restart() {
        // A long line doesn't interrupt itself: the second pitch is taken
        // without restarting the attack — energy doesn't fall to zero, the
        // pitch arrives at the new note.
        let sr = 44_100.0;
        let rms30 =
            |buf: &[f32]| (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        // Fresh attack: the first 30 ms are quieter than the sustain (ramp from zero).
        let mut fresh = BassGuitar::new(1);
        fresh.set_sample_rate(sr);
        fresh.trigger(&NoteEvent::new(1, 33, 0.8, 1.0));
        let mut fresh_buf = Vec::new();
        for _ in 0..(0.03 * sr) as usize {
            fresh_buf.push(fresh.next_sample());
        }
        // Sustain before the transition.
        let mut sus_buf = Vec::new();
        for _ in 0..(0.17 * sr) as usize {
            sus_buf.push(fresh.next_sample());
        }
        let sus = rms30(&sus_buf);
        let fresh_start = rms30(&fresh_buf);
        assert!(fresh_start < sus * 0.9, "attack does not ramp?");
        // Legato to another pitch: no dip, no step.
        let mut leg = BassGuitar::new(1);
        leg.set_sample_rate(sr);
        leg.trigger(&NoteEvent::new(1, 33, 0.8, 1.0));
        for _ in 0..(0.2 * sr) as usize {
            leg.next_sample();
        }
        leg.trigger(&NoteEvent::new(1, 36, 0.8, 1.0));
        let mut leg_buf = Vec::new();
        let mut prev = 0.0f32;
        let mut max_step = 0.0f32;
        for _ in 0..(0.03 * sr) as usize {
            let s = leg.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
            leg_buf.push(s);
        }
        let leg_start = rms30(&leg_buf);
        assert!(
            leg_start > sus * 0.5,
            "legato dips: {leg_start} vs sustain {sus}"
        );
        assert!(max_step < 0.5, "legato step: {max_step}");
        // Pitch has arrived: "late" is already the new note (C2 ~65 Hz ± tolerance).
        let mut late = Vec::new();
        for _ in 0..(0.3 * sr) as usize {
            late.push(leg.next_sample());
        }
        let tail = &late[late.len() - 4096..];
        let peak_near = |f0: f32| {
            let n = tail.len() as f32;
            let k = (f0 / sr * n).round();
            let om = 2.0 * std::f32::consts::PI * k / n;
            let c = 2.0 * om.cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for &v in tail {
                let s0 = v + c * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            (s1 * s1 + s2 * s2 - c * s1 * s2).sqrt()
        };
        assert!(
            peak_near(65.4) > peak_near(55.0),
            "legato never arrives at the new pitch"
        );
    }

    #[test]
    fn retrigger_has_no_audible_step() {
        // Retriggering into a loud sustain (16th-note drops) must not give
        // a step: the tail fades out via crossfade over the new attack.
        let sr = 44_100.0;
        let mut bass = BassGuitar::new(1);
        bass.set_sample_rate(sr);
        bass.trigger(&NoteEvent::new(1, 36, 1.0, 0.5));
        for _ in 0..5000 {
            bass.next_sample();
        }
        bass.trigger(&NoteEvent::new(1, 36, 1.0, 0.5));
        let mut prev = bass.next_sample();
        assert!(prev.is_finite());
        let mut max_step: f32 = 0.0;
        for _ in 0..500 {
            let s = bass.next_sample();
            assert!(s.is_finite());
            max_step = max_step.max((s - prev).abs());
            prev = s;
        }
        assert!(max_step < 0.5, "retrigger step: {max_step}");
    }
}
