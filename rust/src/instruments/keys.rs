use std::f32::consts::TAU;

use super::{
    dsp::{fast_sin, note_seed, release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

const INDEX_FLOOR: f32 = 0.15;
const INDEX_DECAY: f32 = 6.0;
const TINE_RATIO: f32 = 3.98;
const TINE_DECAY: f32 = 45.0;
const TREMOLO_HZ: f32 = 4.5;
const TREMOLO_DEPTH: f32 = 0.22;
/// 8 ms attack (stab spec 5–15): soft entry without a click.
/// 180 Hz high-pass: the low end under the Rhodes is cut (room for bass and kick).
const ATTACK_SECS: f32 = 0.008;
const HP_HZ: f32 = 180.0;
const AMP_DECAY_RATE: f32 = 3.5;
const SUSTAIN_LEVEL: f32 = 0.55;
const FADE_SECS: f32 = 0.08;
const VOICE_GAIN: f32 = 0.65;
/// Polishing lowpass: cutoff holds a constant number of harmonics
/// (12×freq, ceiling 9.5 kHz) — digital shine from FM sidebands is tamed
/// at high notes, while low notes don't go dark.
const POLISH_HZ: f32 = 9500.0;
const POLISH_Q: f32 = 0.7;
const POLISH_HARMS: f32 = 12.0;
const POLISH_FLOOR_HZ: f32 = 1800.0;
/// Hammer fades upward: the 3.98× tine on high notes goes past
/// Nyquist and folds back — its gain melts above 3 kHz of ring.
const TINE_FULL_HZ: f32 = 3000.0;
const TINE_GONE_HZ: f32 = 6000.0;
const TINE_FLOOR: f32 = 0.15;
/// Soft electronic "key" transient (NOT an acoustic hammer:
/// the lofi style explicitly wants no felt/wood). Narrow 1.8 kHz band,
/// 3 ms, very quiet — just so the attack doesn't start as a bare sine.
const CLICK_HZ: f32 = 1800.0;
const CLICK_DECAY: f32 = 320.0;
const CLICK_GAIN: f32 = 0.035;

/// Rhodes-style electric piano voice: FM body with a decaying index
/// (attack "bites", sustain is a pure sine) + a metallic "tine"
/// at 3.98× frequency + gentle tremolo.
#[derive(Clone, Default)]
pub struct KeysVoice {
    sample_rate: f32,
    car_phase: f32,
    mod_phase: f32,
    tine_phase: f32,
    trem_phase: f32,
    polish: Svf,
    frequency: f32,
    gain: f32,
    /// Starting FM depth and hammer ring depend on strike force:
    /// quiet note — pure tone, loud — a barking attack ring.
    /// That's how a real Rhodes behaves.
    index_start: f32,
    tine_gain: f32,
    t: f32,
    samples_left: usize,
    attack_mul: f32,
    release_mul: f32,
    hp: Svf,
    /// "Key" noise: reseeded by note (identical hits are identical).
    noise: Noise,
    click_bp: Svf,
    /// Recursive envelopes instead of per-sample `exp`.
    amp_env: ExpDecay,
    index_env: ExpDecay,
    tine_env: ExpDecay,
    click_env: ExpDecay,
}

impl Voice for KeysVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        self.frequency = midi_to_freq(note.midi_pitch);
        self.samples_left = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.gain = note.velocity;
        // FM index is bitten at the top (max 2.8 instead of 4.2): sidebands
        // stay within the polish band, foldback doesn't whistle.
        // The hammer's bite is intact.
        self.index_start = 1.2 + 1.6 * note.velocity;
        // Hammer ring melts with pitch: up high the tine folds back.
        let tine_hz = self.frequency * TINE_RATIO;
        let tine_fade =
            ((TINE_GONE_HZ - tine_hz) / (TINE_GONE_HZ - TINE_FULL_HZ)).clamp(TINE_FLOOR, 1.0);
        self.tine_gain = (0.12 + 0.45 * note.velocity) * tine_fade;
        self.attack_mul = note.attack_mul;
        self.release_mul = note.release_mul;
        let seed = note_seed(note, 0x4E4D);
        self.noise = Noise::new(seed);
        self.amp_env = ExpDecay::start(AMP_DECAY_RATE, self.sample_rate);
        self.index_env = ExpDecay::start(INDEX_DECAY, self.sample_rate);
        self.tine_env = ExpDecay::start(TINE_DECAY, self.sample_rate);
        self.click_env = ExpDecay::start(CLICK_DECAY, self.sample_rate);
        self.click_bp.reset();
        self.hp.reset();
        self.t = 0.0;
        self.car_phase = 0.0;
        self.mod_phase = 0.0;
        self.tine_phase = 0.0;
        self.polish.scrub_denormals();
        // Per-voice LFO: each voice's tremolo is offset from the note seed,
        // otherwise the whole chord breathes in sync (one-LFO-on-a-pedal effect).
        self.trem_phase = ((seed >> 11) % 1000) as f32 / 1000.0;
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        // Seed-based attack/release: a Rhodes can be biting or mushy.
        let attack_secs = ATTACK_SECS * self.attack_mul.max(0.25);
        let attack_raw = (self.t / attack_secs).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let amp = SUSTAIN_LEVEL + (1.0 - SUSTAIN_LEVEL) * self.amp_env.next();
        let fade_len = (FADE_SECS * self.release_mul.max(0.25) * sample_rate) as usize;
        let fade = release_fade(self.samples_left, fade_len);

        // 1:1 FM with a falling index: bell on attack, sine in sustain.
        let index = INDEX_FLOOR + (self.index_start - INDEX_FLOOR) * self.index_env.next();
        self.mod_phase = (self.mod_phase + self.frequency / sample_rate) % 1.0;
        self.car_phase = (self.car_phase + self.frequency / sample_rate) % 1.0;
        let body = fast_sin(self.car_phase * TAU + index * sin01(self.mod_phase));

        // The hammer's metallic "tine".
        self.tine_phase = (self.tine_phase + (self.frequency * TINE_RATIO) / sample_rate) % 1.0;
        let tine = sin01(self.tine_phase) * self.tine_env.next() * self.tine_gain;

        // There is no acoustic hammer here (felt, wood): this is an
        // electric piano, not a grand. But we keep the electronic "key" —
        // a narrow 3 ms tick — so the attack is no longer a bare sine.
        let (_, click_band, _) =
            self.click_bp
                .process(self.noise.sample(), CLICK_HZ, 1.2, sample_rate);
        let click = click_band * self.click_env.next() * CLICK_GAIN;

        self.trem_phase = (self.trem_phase + TREMOLO_HZ / sample_rate) % 1.0;
        let tremolo = 1.0 + TREMOLO_DEPTH * sin01(self.trem_phase);

        self.t += 1.0 / sample_rate;
        let raw = (body * 0.75 + tine + click) * attack;
        let raw = raw * self.gain * amp * fade * tremolo;
        // 180 Hz high-pass: the low end under the Rhodes is cut, room for bass and kick.
        let (_, _, hipped) = self.hp.process(raw, HP_HZ, 0.7, sample_rate);
        // Pitch-dependent polish: a constant number of harmonics instead
        // of a fixed 9.5 kHz — high notes don't whistle.
        let polish_cut = (self.frequency * POLISH_HARMS).clamp(POLISH_FLOOR_HZ, POLISH_HZ);
        let (polished, _, _) = self
            .polish
            .process(hipped, polish_cut, POLISH_Q, sample_rate);
        polished * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.polish.reset();
        self.hp.reset();
        self.click_bp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    #[test]
    fn seed_articulation_softens_attack() {
        // A soft Rhodes (long attack) bites less in the first 15 ms.
        fn early_energy(atk: f32) -> f32 {
            let sr = 44_100.0;
            let mut keys = VoicePool::keys(7, 1);
            keys.set_sample_rate(sr);
            keys.trigger(&NoteEvent::new(7, 60, 0.7, 0.5).shaped(atk, 1.0));
            (0..(0.015 * sr) as usize)
                .map(|_| {
                    let s = keys.next_sample();
                    assert!(s.is_finite());
                    s.abs()
                })
                .sum()
        }
        let sharp = early_energy(0.8);
        let soft = early_energy(1.8);
        assert!(
            soft < sharp,
            "articulation ignored: soft={soft} sharp={sharp}"
        );
    }

    #[test]
    fn keys_ring_with_tine_and_tremolo() {
        let sr = 44_100.0;
        let mut keys = VoicePool::keys(7, 8);
        keys.set_sample_rate(sr);
        // Jazz chord: root + seventh.
        for pitch in [48, 55, 58, 62] {
            keys.trigger(&NoteEvent::new(7, pitch, 0.7, 1.5));
        }
        let mut peak: f32 = 0.0;
        for _ in 0..(sr as usize) {
            let s = keys.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.2, "keys too quiet: {peak}");
        assert!(keys.is_active(), "long chord cut too early");
    }

    #[test]
    fn attack_fits_stab_spec() {
        // 8 ms attack (spec 5–15): 90% of peak between 5 and 30 ms.
        let sr = 44_100.0;
        let mut keys = VoicePool::keys(7, 1);
        keys.set_sample_rate(sr);
        keys.trigger(&NoteEvent::new(7, 60, 0.8, 0.5));
        let mut peak: f32 = 0.0;
        let buf: Vec<f32> = (0..(0.2 * sr) as usize)
            .map(|_| {
                let s = keys.next_sample();
                assert!(s.is_finite());
                peak = peak.max(s.abs());
                s
            })
            .collect();
        let t90 = buf.iter().position(|s| s.abs() >= peak * 0.9).unwrap();
        assert!(
            (0.005 * sr) as usize <= t90 && t90 <= (0.030 * sr) as usize,
            "attack out of spec: {t90}"
        );
    }
}
