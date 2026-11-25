use super::{
    dsp::{fast_tanh, release_fade, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Electric guitar: a Karplus-Strong string + a gentle burst + light
/// drive. A sharp attack from the string itself (not a click — this isn't
/// acoustic): bright excitation, a tight front, the top dies on its own
/// as the string rings. From short chops to long sustained lines — the
/// range is controlled by note duration: short ones are single strikes,
/// long chords are strummed. For lofi strumming over Rhodes and rock riffs.
///
/// The timbre is an amp cabinet, not a "raw" piezo: first a narrow midrange,
/// then saturation, and only then a final top cut. Drive harmonics don't
/// sit above the cabinet (they were exactly what gave rasp and harshness),
/// while the string keeps its attack and sustain.
const RING_T60_SECS: f32 = 1.3;
const RING_T60_MUTED_SECS: f32 = 0.11;
const MAX_KS_LEN: usize = 2048;
const LP_HZ: f32 = 2500.0;
const LP_MUTED_HZ: f32 = 1400.0;
const LP_Q: f32 = 0.62;
/// Cabinet after saturation: 12 dB/oct plus one more gentle cut.
const CAB_HZ: f32 = 3800.0;
const CAB_MUTED_HZ: f32 = 1900.0;
const CAB_Q: f32 = 0.58;
const ATTACK_SECS: f32 = 0.008;
/// Below this — a ghost note with muted strings.
const MUTE_VELOCITY: f32 = 0.6;
/// Strumming: strike offsets and their accents (the sum is normalized).
/// 16 ms between strings — strokes are heard separately, like strumming,
/// rather than one hit with tremolo.
const STRUM_GAP_SECS: f32 = 0.016;
const STRUM_GAP_MUTED_SECS: f32 = 0.008;
const STRUM_ACCENTS: [f32; 3] = [1.0, 0.7, 0.5];
const STRUM_NORM: f32 = 1.0 / (1.0 + 0.7 + 0.5);
const MAX_RING_SECS: f32 = 1.5;
const FADE_SECS: f32 = 0.05;
/// Output normalized for the mix: the string is quieter than the synth but
/// audible (test pin). The string is the riff (in rock it leads), so the level
/// sits near the snare: KS averaging tames the top, and without compensation
/// the riff sat 6 dB under the drums.
const VOICE_GAIN: f32 = 1.85;
/// Lead character: longer ring (sustain line), open top (legibility of
/// picking), light drive (singing compression instead of a flat ring).
/// Riff character: tight burst, drive on the chops.
const LEAD_RING_T60_SECS: f32 = 2.4;
const LEAD_LP_HZ: f32 = 3200.0;
const LEAD_CAB_HZ: f32 = 4700.0;
const LEAD_DRIVE: f32 = 1.3;
/// Note-length ceiling for lead: sustained lines aren't cut off at 1.5 s.
const LEAD_MAX_RING_SECS: f32 = 3.0;
const RIFF_DRIVE: f32 = 1.12;
/// Excitation brightness (one-pole burst alpha). It was 0.7/0.85:
/// noise reached 5 kHz and rang after drive. Now the excitation is
/// darker — the string's top is born in the KS ring, not in the noise click.
const RIFF_BURST_ALPHA: f32 = 0.5;
const LEAD_BURST_ALPHA: f32 = 0.62;
/// Pick-position comb: the pick sits at 1/5 of the string → a null on the
/// 5th harmonic. Removes the "synthetic" flat spectrum of KS without samples.
const PICK_POSITION_DIV: usize = 5;
const PICK_COMB_GAIN: f32 = 0.28;

#[derive(Clone)]
pub struct GuitarVoice {
    sample_rate: f32,
    ks_buf: Vec<f32>,
    ks_pos: usize,
    ks_len: usize,
    /// Fractional part of the period in samples: an integer length gave
    /// detuning up to ±0.5% (~5 cents at E2). The allpass supplies the rest.
    frac: f32,
    /// State of the fractional-delay allpass interpolator.
    ap_x: f32,
    ap_y: f32,
    /// Offset for the pick-position comb (in line samples).
    pick_offset: usize,
    /// Gentle stereo offset of the voice (±0.25), determined by the note seed.
    pan: f32,
    damp: f32,
    filter: Svf,
    /// Cabinet after saturation: a second cut that takes drive harmonics
    /// above 3.8–4.7 kHz.
    cab: Svf,
    noise: Noise,
    frequency: f32,
    gain: f32,
    muted: bool,
    strums: bool,
    t: f32,
    samples_left: usize,
    /// Character: string sustain, top cut and output drive.
    /// Riff — dry and tight, lead — longer, brighter, with gentle
    /// saturation (a singing sustain line instead of a flat ring).
    ring_t60: f32,
    lp_hz: f32,
    cab_hz: f32,
    drive: f32,
    /// Excitation burst brightness (see RIFF/LEAD_BURST_ALPHA).
    burst_alpha: f32,
    /// Note-length ceiling, s: short chops stop early,
    /// long lines stretch out (sustained lead sounds).
    max_ring: f32,
}

impl GuitarVoice {
    /// Riff: short chops, palm mute, biting burst, tight drive.
    pub fn riff() -> Self {
        Self {
            sample_rate: 44_100.0,
            ks_buf: Vec::new(),
            ks_pos: 0,
            ks_len: 0,
            frac: 0.0,
            ap_x: 0.0,
            ap_y: 0.0,
            pick_offset: 2,
            pan: 0.0,
            damp: 0.0,
            filter: Svf::default(),
            cab: Svf::default(),
            noise: Noise::default(),
            frequency: 0.0,
            gain: 0.0,
            muted: false,
            strums: false,
            t: 0.0,
            samples_left: 0,
            ring_t60: RING_T60_SECS,
            lp_hz: LP_HZ,
            cab_hz: CAB_HZ,
            drive: RIFF_DRIVE,
            burst_alpha: RIFF_BURST_ALPHA,
            max_ring: MAX_RING_SECS,
        }
    }

    /// Lead/picking: long ring, open top, singing drive.
    pub fn lead() -> Self {
        let mut v = Self::riff();
        v.ring_t60 = LEAD_RING_T60_SECS;
        v.lp_hz = LEAD_LP_HZ;
        v.cab_hz = LEAD_CAB_HZ;
        v.drive = LEAD_DRIVE;
        v.burst_alpha = LEAD_BURST_ALPHA;
        v.max_ring = LEAD_MAX_RING_SECS;
        v
    }

    /// One mono string sample. Shared by `next_sample` and the stereo output:
    /// pan is applied to the already-computed signal, not to the KS state.
    fn render(&mut self) -> f32 {
        if self.samples_left == 0 || self.ks_len == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        // Strumming only on accents: a fan of three strikes. A normal strike is
        // a single pluck with the same attack (otherwise fingerpicking doubles).
        let gap = if self.muted {
            STRUM_GAP_MUTED_SECS
        } else {
            STRUM_GAP_SECS
        };
        let mut strum = 0.0;
        if self.strums {
            for (k, accent) in STRUM_ACCENTS.iter().enumerate() {
                let tau = self.t - k as f32 * gap;
                if tau > 0.0 {
                    let u = (tau / ATTACK_SECS).min(1.0);
                    strum += accent * u * u * (3.0 - 2.0 * u);
                }
            }
            strum *= STRUM_NORM;
        } else {
            let u = (self.t / ATTACK_SECS).min(1.0);
            strum = u * u * (3.0 - 2.0 * u);
        }

        // Karplus-Strong with fractional delay. Integer part of the period —
        // a ring buffer, fractional part — a one-pole allpass:
        //   y[n] = a·x[n] + x[n-1] − a·y[n-1],  a = (1−f)/(1+f)
        // Its DC delay is exactly f samples, |H|=1 — tuning is
        // exact without changing loudness or adding an LP.
        let cur = self.ks_buf[self.ks_pos];
        let nxt = self.ks_buf[(self.ks_pos + 1) % self.ks_len];
        let pick_tap = self.ks_buf[(self.ks_pos + self.pick_offset) % self.ks_len];
        let a_ap = (1.0 - self.frac) / (1.0 + self.frac);
        let delayed = a_ap * cur + self.ap_x - a_ap * self.ap_y;
        self.ap_x = cur;
        self.ap_y = delayed;
        // String output — with a comb from the pick position (null on 5th harmonic).
        let pluck = delayed - PICK_COMB_GAIN * pick_tap;
        self.ks_buf[self.ks_pos] = self.damp * 0.5 * (delayed + nxt);
        self.ks_pos = (self.ks_pos + 1) % self.ks_len;

        let cutoff = if self.muted { LP_MUTED_HZ } else { self.lp_hz };
        let (warm, _, _) = self.filter.process(pluck, cutoff, LP_Q, sample_rate);
        // Saturation BEFORE the cabinet: drive harmonics are born, but at once
        // go under the speaker's cut (otherwise harshness at 2–5 kHz).
        let driven = fast_tanh(warm * self.drive);
        let cab_hz = if self.muted {
            CAB_MUTED_HZ
        } else {
            self.cab_hz
        };
        let (voiced, _, _) = self.cab.process(driven, cab_hz, CAB_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        voiced * self.gain * attack * fade * strum * VOICE_GAIN
    }
}

impl Default for GuitarVoice {
    fn default() -> Self {
        Self::riff()
    }
}

impl Voice for GuitarVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let sample_rate = self.sample_rate;
        // Humanization of tuning and ring from the note: ±4 cents and ±10% sustain.
        // A live string is slightly different on every strike (machine
        // sameness = half of the "synthetic" feel). Determined by the note.
        let seed = super::dsp::note_seed(note, 0x6174);
        let detune = 1.0 + ((seed % 200) as f32 / 100.0 - 1.0) * 0.0023;
        let ring_var = 0.9 + 0.2 * (((seed >> 9) % 100) as f32 / 100.0);
        self.frequency = midi_to_freq(note.midi_pitch) * detune;
        let ring = note.duration_secs.clamp(0.05, self.max_ring);
        self.samples_left = (ring * sample_rate) as usize;
        self.gain = note.velocity;
        // Quiet ghost notes are played with muted strings.
        self.muted = note.velocity < MUTE_VELOCITY;
        // Strum only long accented chords: short
        // chops go as single strikes (otherwise every 16th chop sounds like a
        // 32 ms doublet and the picking blurs).
        self.strums = note.velocity > 0.75 && note.duration_secs >= 0.3;
        self.t = 0.0;
        // String: a delay line of one pitch period + decay toward T60.
        // Muted strings ring 0.12 s (palm), open ones — 1.1 s.
        // FRACTIONAL delay: integer part → buffer, fractional → allpass.
        // Rounding used to give detuning up to a semitone on high notes.
        let period = sample_rate / self.frequency.max(20.0);
        self.ks_len = (period.floor() as usize).clamp(8, MAX_KS_LEN);
        self.frac = if period >= 8.0 && period <= MAX_KS_LEN as f32 {
            (period - self.ks_len as f32).clamp(0.0, 0.999)
        } else {
            0.0
        };
        self.ap_x = 0.0;
        self.ap_y = 0.0;
        self.pick_offset = (self.ks_len / PICK_POSITION_DIV).max(2);
        // Deterministic stereo offset: 2–3 strings of a chord sit slightly
        // left/right of center (no hard pan — the fold to mono is even).
        let p = ((seed >> 16) % 1000) as f32 / 1000.0;
        self.pan = (p * 2.0 - 1.0) * 0.25;
        let t60 = if self.muted {
            RING_T60_MUTED_SECS
        } else {
            // Ring humanization: sustain breathes from note to note.
            self.ring_t60 * ring_var
        };
        self.damp = (-6.908 * self.ks_len as f32 / (t60 * sample_rate)).exp();
        // Excitation pluck: noise through a gentle lowpass (brightness is set by
        // the character). The burst is stable within a velocity bucket
        // (otherwise energy jumps twofold from strike to strike); only tuning
        // and ring breathe (below) — alive, but not sloppy.
        // Normalize to full scale: softness comes from the filter, not silence.
        let vbits = note.velocity.to_bits();
        self.noise = Noise::new(0x6174 ^ (note.midi_pitch as u32).wrapping_mul(0x9E37) ^ vbits);
        self.ks_buf.resize(self.ks_len, 0.0);
        // Excitation: noise through a gentle lowpass — a biting front, the top
        // dies on its own during the ring (KS averaging). Alpha 0.5 used to be —
        // a dull strike with no highs (half a "kalimba").
        let alpha = self.burst_alpha;
        let mut lp = 0.0f32;
        let mut peak = 0.0f32;
        for b in self.ks_buf.iter_mut() {
            let n = self.noise.sample();
            lp += alpha * (n - lp);
            *b = lp;
            peak = peak.max(lp.abs());
        }
        if peak > 1e-6 {
            let norm = 0.9 / peak;
            for b in self.ks_buf.iter_mut() {
                *b *= norm;
            }
        }
        self.ks_pos = 0;
        self.filter.reset();
        self.cab.reset();
    }

    fn next_sample(&mut self) -> f32 {
        self.render()
    }

    fn next_sample_stereo(&mut self) -> (f32, f32) {
        let s = self.render();
        let p = self.pan;
        (s * (1.0 - p), s * (1.0 + p))
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.filter.reset();
        self.cab.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{band_ratio, Instrument, VoicePool};
    use super::*;

    #[test]
    fn guitars_keep_harsh_band_down() {
        // Guitar is a cabinet, not a piezo: riff and lead live in the mids,
        // 2–5 kHz shouldn't dominate. That band was exactly what gave
        // rasp and harshness on a phone.
        let sr = 44_100.0;
        let probe = |lead: bool, pitch: u8| -> Vec<f32> {
            let mut pool = if lead {
                VoicePool::guitar_leads(20, 1)
            } else {
                VoicePool::guitars(14, 1)
            };
            pool.set_sample_rate(sr);
            pool.trigger(&NoteEvent::new(14, pitch, 0.9, 1.0));
            (0..(1.0 * sr) as usize)
                .map(|_| pool.next_sample())
                .collect()
        };
        for (name, lead, pitch) in [("riff", false, 64u8), ("lead", true, 57u8)] {
            let out = probe(lead, pitch);
            let harsh = band_ratio(&out, sr, 2000.0, 5000.0);
            let air = band_ratio(&out, sr, 5000.0, 10_000.0);
            let body = band_ratio(&out, sr, 500.0, 2000.0);
            assert!(harsh < 0.10, "{name} harsh band: {harsh:.4}");
            assert!(air < 0.01, "{name} air band: {air:.4}");
            assert!(body > 0.2, "{name} lost its body: {body:.4}");
        }
    }

    fn guitars() -> VoicePool {
        VoicePool::guitars(16, 6)
    }

    #[test]
    fn short_chops_dont_fan() {
        // Short chops are single strikes (a 32 ms fan on every 16th
        // smears fast picking), long chords are strummed.
        fn is_strum(dur: f32) -> bool {
            let mut v = GuitarVoice::riff();
            v.set_sample_rate(44_100.0);
            v.trigger(&NoteEvent::new(14, 57, 0.9, dur));
            v.strums
        }
        assert!(!is_strum(0.1), "chop fanned");
        assert!(is_strum(1.0), "long chord not strummed");
    }

    #[test]
    fn lead_sustains_longer_than_riff() {
        // Lead holds the ring longer than the riff (sustain line vs chop):
        // at the same note, the lead's second-half energy is higher.
        fn late_energy(lead: bool) -> f32 {
            let sr = 44_100.0;
            let mut pool = if lead {
                VoicePool::guitar_leads(20, 1)
            } else {
                VoicePool::guitars(14, 1)
            };
            pool.set_sample_rate(sr);
            pool.trigger(&NoteEvent::new(14, 57, 0.9, 1.5));
            let mut sum = 0.0;
            for i in 0..(1.5 * sr) as usize {
                let s = pool.next_sample();
                assert!(s.is_finite());
                if i >= (0.75 * sr) as usize {
                    sum += s.abs();
                }
            }
            sum
        }
        let riff = late_energy(false);
        let lead = late_energy(true);
        assert!(lead > riff, "solo decays like riff: {lead} vs {riff}");
    }

    #[test]
    fn guitar_plucks_warm() {
        let sr = 44_100.0;
        let mut guitar = guitars();
        guitar.set_sample_rate(sr);
        guitar.trigger(&NoteEvent::new(16, 64, 0.9, 1.0));
        let mut peak: f32 = 0.0;
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..(sr as usize) {
            let s = guitar.next_sample();
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            if i < (0.5 * sr) as usize {
                first += s.abs();
            } else {
                second += s.abs();
            }
        }
        assert!(peak > 0.15, "guitar too quiet: {peak}");
        assert!(second < first, "guitar does not decay");
    }

    #[test]
    fn trigger_humanizes_tuning_and_ring() {
        // The same pitch at different velocities (per-strike jitter) —
        // slightly different tuning (±4 cents) and ring; at the same velocity —
        // bit-for-bit. Machine sameness goes away, determinism stays.
        fn ring(vel: f32) -> Vec<f32> {
            let sr = 44_100.0;
            let mut guitar = guitars();
            guitar.set_sample_rate(sr);
            guitar.trigger(&NoteEvent::new(14, 64, vel, 0.5));
            (0..(0.3 * sr) as usize)
                .map(|_| {
                    let s = guitar.next_sample();
                    assert!(s.is_finite());
                    s
                })
                .collect()
        }
        let a = ring(0.85);
        assert_eq!(a, ring(0.85), "same note must repeat");
        let c = ring(0.87);
        assert_ne!(a, c, "humanization dead");
        // Tuning holds: energies within 3 dB.
        let e = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>();
        let ratio = e(&a) / e(&c).max(1e-9);
        assert!((ratio - 1.0).abs() < 0.5, "tuning drifted: {ratio}");
    }

    #[test]
    fn fractional_delay_matches_the_string_period() {
        // An integer length + fractional allpass must produce the exact period:
        // `.round()` used to give detuning up to a semitone up high.
        let sr = 44_100.0;
        for pitch in [40u8, 52, 64, 76] {
            let mut v = GuitarVoice::riff();
            v.set_sample_rate(sr);
            v.trigger(&NoteEvent::new(14, pitch, 0.8, 0.5));
            let period = sr / v.frequency;
            assert!(
                ((v.ks_len as f32 + v.frac) - period).abs() < 0.01,
                "pitch {pitch}: delay {} vs period {period}",
                v.ks_len as f32 + v.frac
            );
            // The allpass starts from zero: no transients on the pluck.
            assert_eq!((v.ap_x, v.ap_y), (0.0, 0.0));
        }
    }

    #[test]
    fn nearby_velocities_get_distinct_noise_seeds() {
        // Quantizing into 4 buckets made 0.50/0.5001 the same strike.
        let render = |vel: f32| {
            let sr = 44_100.0;
            let mut v = GuitarVoice::riff();
            v.set_sample_rate(sr);
            v.trigger(&NoteEvent::new(14, 64, vel, 0.5));
            (0..2000).map(|_| v.next_sample()).collect::<Vec<_>>()
        };
        assert_ne!(render(0.50), render(0.5001));
    }

    #[test]
    fn muted_ghost_dies_fast() {
        // A quiet note (ghost) is muted: near silence in the second half,
        // while an open note in the same window is still sounding.
        let sr = 44_100.0;
        let energy = |vel: f32| {
            let mut guitar = guitars();
            guitar.set_sample_rate(sr);
            guitar.trigger(&NoteEvent::new(16, 64, vel, 1.0));
            let mut late = 0.0;
            for i in 0..(sr as usize) {
                let s = guitar.next_sample();
                assert!(s.is_finite());
                if i >= (0.5 * sr) as usize {
                    late += s.abs();
                }
            }
            late
        };
        let open = energy(0.9);
        let muted = energy(0.4);
        assert!(muted < open * 0.2, "mute does not mute: {muted} vs {open}");
    }
}
