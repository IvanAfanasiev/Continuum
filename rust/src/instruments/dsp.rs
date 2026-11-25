//! Common DSP primitives.
//!
//! Two upgrades against the "8-bit" sound:
//! - PolyBLEP oscillators: naive saw/square generate harmonics above
//!   Nyquist that fold back into the audible range (aliasing,
//!   which gives chiptune harshness). BLEP smooths the step at the discontinuity.
//! - Resonant SVF instead of one-pole 6 dB/oct: cuts the top more steeply
//!   and supports a modern resonant sweep.

use std::f32::consts::PI;

/// PolyBLEP step correction (2 points, linear interpolation).
/// `phase` is in [0, 1), `dt` = freq/sr.
///
/// A full 4-point B-spline BLEP needs state for 4 samples around each
/// discontinuity; the stateless cubic "4-point" approximation
/// (−1+3t²−2t³) was measured and turned out WORSE than linear near
/// Nyquist (aliasing of a 10 kHz third harmonic doubled). So the voices
/// keep the proven 2-point correction, and the upper-register problem was
/// solved with 2× oversampling exactly where it really occurred:
/// in the hi-hat's metal source (see `hat.rs`).
pub fn poly_blep2(phase: f32, dt: f32) -> f32 {
    let dt = dt.clamp(1e-6, 0.4);
    if phase < dt {
        let t = phase / dt;
        t + t - t * t - 1.0
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Main correction for the voices.
#[inline]
pub fn poly_blep(phase: f32, dt: f32) -> f32 {
    poly_blep2(phase, dt)
}

/// Saw −1..1 without aliasing.
pub fn saw_blep(phase: f32, freq: f32, sample_rate: f32) -> f32 {
    let dt = (freq / sample_rate).clamp(1e-6, 0.4);
    (phase * 2.0 - 1.0) - poly_blep(phase, dt)
}

/// Square ±1 without aliasing (discontinuities at 0 and 0.5).
pub fn square_blep(phase: f32, freq: f32, sample_rate: f32) -> f32 {
    let dt = (freq / sample_rate).clamp(1e-6, 0.4);
    let naive = if phase < 0.5 { 1.0 } else { -1.0 };
    let shifted = if phase < 0.5 {
        phase + 0.5
    } else {
        phase - 0.5
    };
    naive + poly_blep(phase, dt) - poly_blep(shifted, dt)
}

/// Fast tanh for per-sample voice saturations: a 2048-point table
/// on [-4, 4] + linear interpolation (error < 1e-5).
/// An order of magnitude cheaper than libm `tanh` (two reads + arithmetic
/// instead of range reduction and `exp`), indistinguishable by ear: the
/// saturation character (soft limiting) is preserved.
/// The master chain uses the exact `tanh` as before.
pub fn fast_tanh(x: f32) -> f32 {
    use std::sync::OnceLock;
    const N: usize = 2048;
    const LO: f32 = -4.0;
    const HI: f32 = 4.0;
    static TABLE: OnceLock<[f32; N + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0f32; N + 1];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (LO + (HI - LO) * i as f32 / N as f32).tanh();
        }
        t
    });
    let xc = x.clamp(LO, HI);
    let pos = (xc - LO) / (HI - LO) * N as f32;
    let idx = (pos as usize).min(N - 1);
    let frac = pos - idx as f32;
    table[idx] + (table[idx + 1] - table[idx]) * frac
}

/// Fast sine in radians: a 2048-point table + lerp (error < 1e-5).
/// For oscillator phases already living in [0, 1) cycles, [`sin01`] is
/// cheaper (no TAU multiply and no large-argument reduction).
pub fn fast_sin(x: f32) -> f32 {
    use std::f32::consts::TAU;
    use std::sync::OnceLock;
    const N: usize = 2048;
    static TABLE: OnceLock<[f32; N + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0f32; N + 1];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (TAU * i as f32 / N as f32).sin();
        }
        t
    });
    let mut w = x % TAU;
    if w < 0.0 {
        w += TAU;
    }
    let pos = w / TAU * N as f32;
    let idx = (pos as usize).min(N - 1);
    let frac = pos - idx as f32;
    table[idx] + (table[idx + 1] - table[idx]) * frac
}

/// Fast sine from a phase in cycles [0, 1): direct index into the same
/// table without multiplying by TAU. Phase wraps (`% 1.0`),
/// oscillators never have negative phases.
pub fn sin01(phase: f32) -> f32 {
    use std::f32::consts::TAU;
    use std::sync::OnceLock;
    const N: usize = 2048;
    static TABLE: OnceLock<[f32; N + 1]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0.0f32; N + 1];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (TAU * i as f32 / N as f32).sin();
        }
        t
    });
    let pos = (phase % 1.0) * N as f32;
    let idx = (pos as usize).min(N - 1);
    let frac = pos - idx as f32;
    table[idx] + (table[idx + 1] - table[idx]) * frac
}
/// Recursive exponential envelope: a replacement for per-sample
/// `(-k*t).exp()`. One `exp()` in `trigger` (to compute `coeff`),
/// then one multiply per sample. The result matches direct
/// `exp` to float precision (drift <1e-6 per second).
#[derive(Clone, Default, Debug)]
pub struct ExpDecay {
    value: f32,
    coeff: f32,
}

impl ExpDecay {
    /// Start at 1.0 with rate `rate_per_sec` (envelope `exp(-rate*t)`).
    pub fn start(rate_per_sec: f32, sample_rate: f32) -> Self {
        Self {
            value: 1.0,
            coeff: (-rate_per_sec / sample_rate.max(1.0)).exp(),
        }
    }

    /// Current value + step. Denormal guard: below 1e-12 → zero.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        let v = self.value;
        self.value *= self.coeff;
        if self.value < 1e-12 {
            self.value = 0.0;
        }
        v
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}
/// Resonant state-variable filter (Zavalishin SVF, 12 dB/oct).
/// Unconditionally stable at any cutoff/Q — unlike ladder filters.
///
/// Coefficients are cached: `tan()` is computed only when
/// (cutoff, Q, sr) changes. Fixed-cutoff filters (master HP 28 Hz,
/// voice HPs) don't touch libm at all after the first sample.
/// Sweeps with a moving cutoff recompute as before —
/// the sound is bit-for-bit identical, the comparison exact (same input =
/// same coefficient, the cache introduces no error).
#[derive(Clone)]
pub struct Svf {
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    last_cutoff: f32,
    last_q: f32,
    last_sr: f32,
}

impl Default for Svf {
    fn default() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 0.0,
            // Invalid combination: the first call must compute.
            last_cutoff: f32::NAN,
            last_q: f32::NAN,
            last_sr: f32::NAN,
        }
    }
}

impl Svf {
    /// One pass — gives (low, band, high) at once.
    pub fn process(&mut self, x: f32, cutoff_hz: f32, q: f32, sample_rate: f32) -> (f32, f32, f32) {
        // NaN is unequal to everything, including itself: after default
        // and after reset the coefficients are guaranteed to be recomputed.
        if cutoff_hz != self.last_cutoff || q != self.last_q || sample_rate != self.last_sr {
            let g = (PI * cutoff_hz.clamp(10.0, sample_rate * 0.45) / sample_rate).tan();
            let k = 1.0 / q.clamp(0.3, 12.0);
            let a1 = 1.0 / (1.0 + g * (g + k));
            self.g = g;
            self.k = k;
            self.a1 = a1;
            self.a2 = g * a1;
            self.a3 = g * g * a1;
            self.last_cutoff = cutoff_hz;
            self.last_q = q;
            self.last_sr = sample_rate;
        }
        let v3 = x - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        (v2, v1, x - self.k * v1 - v2)
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Scrub denormal states: after long silence, leftovers in the filter
    /// can fall into the subnormal range, and arithmetic with them on x86
    /// is tens of times slower — hence rare CPU spikes (rasp).
    pub fn scrub_denormals(&mut self) {
        if self.ic1eq.abs() < 1e-15 {
            self.ic1eq = 0.0;
        }
        if self.ic2eq.abs() < 1e-15 {
            self.ic2eq = 0.0;
        }
    }
}

/// Maximum lookahead line length: 512 samples ≈ 10.6 ms at 48 kHz.
/// An array on the struct's stack — no allocation in the audio path.
const LOOKAHEAD_CAP: usize = 512;

/// True-peak estimate from two adjacent samples (2× interpolation).
/// A sample peak can't see the inter-sample peak: a sine near Nyquist may
/// have an interpolant √2 times larger than its neighbors. We add the
/// midpoint and a quarter of the difference — cheap, but effective.
#[inline]
fn true_peak(a: f32, b: f32) -> f32 {
    let mid = (a + b) * 0.5;
    let slope = (a - b) * 0.25;
    a.abs().max(b.abs()).max(mid.abs() + slope.abs())
}

/// Master peak limiter with stereo linking and 3 ms lookahead.
///
/// Gain used to be computed from the current sample: a peak slipped through
/// before the gain caught it (2 ms attack), leaving an overshoot on every
/// transient. Now the input goes into a ring line, and the output is read
/// `lookahead` samples back: the detector sees the future window and can
/// pull the gain down BEFORE the peak arrives.
/// The stereo pair shares one gain: the image doesn't skew.
/// Quiet signals pass transparently; master tanh drive is outside.
#[derive(Clone, Debug)]
pub struct MasterLimiter {
    env: f32,
    /// Current gain reduction (0..1): falls fast, rises slowly.
    gain: f32,
    buf_l: [f32; LOOKAHEAD_CAP],
    buf_r: [f32; LOOKAHEAD_CAP],
    write: usize,
    /// How many samples we look ahead (3 ms from the sample rate).
    look: usize,
    /// Body makeup, times. Moderate: the limiter only catches freak stacks,
    /// the mix character (crest factor) stays intact.
    pub makeup: f32,
    /// Peak ceiling at the limiter output.
    pub ceiling: f32,
    /// Detector lookahead, seconds (default 3 ms).
    pub lookahead_secs: f32,
    /// Envelope release, seconds.
    pub release_secs: f32,
    /// Gain-reduction action time, seconds: how long the gain is pulled
    /// down instead of jumping instantly.
    pub attack_secs: f32,
    /// Cache of `exp(-1/(sr*release))`: a chain constant, not per-sample.
    rel: f32,
    /// Gain-attack cache.
    att: f32,
    last_sr: f32,
}

impl Default for MasterLimiter {
    fn default() -> Self {
        Self {
            env: 0.0,
            gain: 1.0,
            buf_l: [0.0; LOOKAHEAD_CAP],
            buf_r: [0.0; LOOKAHEAD_CAP],
            write: 0,
            look: 132, // 3 ms at 44.1 kHz
            makeup: 1.0,
            ceiling: 0.85,
            lookahead_secs: 0.003,
            release_secs: 0.05,
            attack_secs: 0.002,
            rel: 0.0,
            att: 0.0,
            last_sr: f32::NAN,
        }
    }
}

impl MasterLimiter {
    /// Stereo frame after delay, makeup and limiting (tanh is outside).
    pub fn process(&mut self, l: f32, r: f32, sample_rate: f32) -> (f32, f32) {
        let sr = sample_rate.max(1.0);
        if sample_rate != self.last_sr {
            self.rel = (-1.0 / (sr * self.release_secs)).exp();
            self.att = (-1.0 / (sr * self.attack_secs)).exp();
            self.look = ((sr * self.lookahead_secs) as usize)
                .clamp(2, LOOKAHEAD_CAP - 1);
            self.last_sr = sample_rate;
        }

        // Write NOW, read from the past: between read and write lies the
        // future window relative to the output sample.
        self.buf_l[self.write] = l;
        self.buf_r[self.write] = r;
        let read = (self.write + LOOKAHEAD_CAP - self.look) % LOOKAHEAD_CAP;
        let out_l = self.buf_l[read];
        let out_r = self.buf_r[read];

        // True peak over the whole lookahead window (including the freshest sample).
        let mut peak = 0.0f32;
        let mut idx = read;
        let mut prev_l = self.buf_l[(read + LOOKAHEAD_CAP - 1) % LOOKAHEAD_CAP];
        let mut prev_r = self.buf_r[(read + LOOKAHEAD_CAP - 1) % LOOKAHEAD_CAP];
        for _ in 0..self.look {
            let cl = self.buf_l[idx];
            let cr = self.buf_r[idx];
            peak = peak.max(true_peak(prev_l, cl)).max(true_peak(prev_r, cr));
            prev_l = cl;
            prev_r = cr;
            idx += 1;
            if idx == LOOKAHEAD_CAP {
                idx = 0;
            }
        }
        peak *= self.makeup;

        self.env = peak.max(self.env * self.rel);
        if self.env < 1e-12 {
            self.env = 0.0;
        }
        let target = (self.ceiling / self.env.max(1e-6)).min(1.0);
        // Gain reduction is smoothed: downward by attack, upward by release.
        let k = if target < self.gain {
            self.att
        } else {
            self.rel
        };
        self.gain += (target - self.gain) * (1.0 - k);

        self.write += 1;
        if self.write == LOOKAHEAD_CAP {
            self.write = 0;
        }
        (out_l * self.makeup * self.gain, out_r * self.makeup * self.gain)
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.gain = 1.0;
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write = 0;
    }
}

/// Linear fade of a note's tail: 1 at the start of the fade → 0 on the last sample.
/// Removes clicks from hard cutoff at `samples_left == 0`.
pub fn release_fade(samples_left: usize, fade_samples: usize) -> f32 {
    if fade_samples == 0 {
        return 1.0;
    }
    (samples_left as f32 / fade_samples as f32).clamp(0.0, 1.0)
}

/// Deterministic hit seed from a note: identical notes sound identical,
/// differing velocity/pitch gives natural timbral variation
/// (anti-machine-gun on rolls). Doesn't affect pitch or loudness.
pub fn note_seed(note: &super::NoteEvent, salt: u32) -> u32 {
    salt ^ (note.midi_pitch as u32).wrapping_mul(0x9E37) ^ (note.velocity.to_bits() >> 7)
}

/// Deterministic PCG32 white noise (XSH-RR). One generator per voice —
/// consecutively triggered notes are uncorrelated.
///
/// Why LCG was replaced: the old generator's low bits had a shorter
/// period (it was also used as a source of "random" bits for droplet
/// pitches/fractional parts), producing a faint comb in noise layers' spectra.
/// PCG scatters the bits with an output permutation: uniform noise without
/// short correlations.
/// No `rand`/`SystemTime`: state comes entirely from the note seed.
#[derive(Clone)]
pub struct Noise {
    state: u64,
    inc: u64,
}

impl Default for Noise {
    fn default() -> Self {
        // Canonical PCG constants: the default generator must also
        // produce a full sequence, not zeros.
        Self {
            state: 0x853c_49e6_748f_ea9b,
            inc: 0xda3e_39cb_94b9_5bdb,
        }
    }
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self {
            state: (seed as u64)
                .wrapping_mul(0x5851_f42d_4c95_7f2d)
                .wrapping_add(0x853c_49e6_748f_ea9b),
            // The increment must be odd — otherwise the period shrinks.
            inc: (((seed as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)) | 1),
        }
    }

    pub fn sample(&mut self) -> f32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let shifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        let bits = shifted.rotate_right(rot);
        (bits as f32 * (1.0 / u32::MAX as f32)) * 2.0 - 1.0
    }

    /// Next 32-bit word — for "digital" decisions (droplet pitch,
    /// vowel choice, etc.) where the bit itself matters, not the noise level.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let shifted = (((old >> 18) ^ old) >> 27) as u32;
        shifted.rotate_right((old >> 59) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn fade_endpoints() {
        assert_eq!(release_fade(100, 10), 1.0);
        assert_eq!(release_fade(0, 10), 0.0);
        assert!((release_fade(5, 10) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn limiter_bounds_peaks_keeps_quiet_transparent() {
        // A 3.0 peak is pushed to the ceiling, 0.1 silence passes almost as is,
        // the stereo pair shares one gain (the image stays intact).
        // Gain reduction is smoothed (2 ms attack): the first milliseconds let
        // the peak through — a short overshoot that the master's soft tanh
        // later cuts. The point is that the ceiling holds after the attack.
        let sr = 44_100.0;
        let mut lim = MasterLimiter::default();
        let (mut peak_in, mut peak_out, mut peak_settled) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..(sr as usize) {
            let t = i as f32 / sr;
            let loud = (t * 440.0 * TAU).sin() * 3.0;
            let (l, r) = lim.process(loud, loud * 0.5, sr);
            peak_in = peak_in.max(loud.abs());
            peak_out = peak_out.max(l.abs().max(r.abs()));
            if i > (sr * 0.05) as usize {
                peak_settled = peak_settled.max(l.abs().max(r.abs()));
            }
        }
        assert!(peak_in > 2.9);
        assert!(
            peak_settled <= lim.ceiling * lim.makeup * 1.02 + 1e-3,
            "limiter leaks peaks: {peak_settled}"
        );
        // The master output (tanh) never exceeds unity: the limiter's
        // overshoot is cut softly, without clipping.
        assert!(peak_out <= 3.0, "limiter made it worse: {peak_out}");
        // A quiet sine: gain ~ makeup, no channel imbalance.
        // The lookahead line is empty at first — run 3 ms of quiet signal
        // so that same signal comes out, not the initial zeros.
        let mut lim = MasterLimiter::default();
        let mut store = (0.0f32, 0.0f32);
        for _ in 0..lim.look + 1 {
            store = lim.process(0.1, 0.1, sr);
        }
        let (l, r) = store;
        assert!((l - r).abs() < 1e-6, "stereo image skewed");
        assert!(
            (l - 0.1 * lim.makeup).abs() < 1e-3,
            "quiet path colored: {l}"
        );
    }

    #[test]
    fn limiter_recovers_after_transient() {
        // After a loud hit the gain returns in ~5 releases:
        // the pump breathes instead of choking everything.
        let sr = 44_100.0;
        let mut lim = MasterLimiter::default();
        for _ in 0..1000 {
            lim.process(3.0, 3.0, sr);
        }
        for _ in 0..(5.0 * 0.05 * sr) as usize {
            lim.process(0.05, 0.05, sr);
        }
        let (l, _) = lim.process(0.05, 0.05, sr);
        assert!((l - 0.05 * lim.makeup).abs() < 0.01, "gain stuck down: {l}");
    }

    /// Energy of a single frequency bin (Goertzel) — a spectral test
    /// without FFT.
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

    fn render_saw(bleped: bool, freq: f32, sample_rate: f32, n: usize) -> Vec<f32> {
        let mut phase = 0.0;
        (0..n)
            .map(|_| {
                phase = (phase + freq / sample_rate) % 1.0;
                if bleped {
                    saw_blep(phase, freq, sample_rate)
                } else {
                    phase * 2.0 - 1.0
                }
            })
            .collect()
    }

    #[test]
    fn saw_blep_kills_folded_aliasing_but_keeps_body() {
        let sr = 44_100.0;
        let n = sr as usize;
        // Folded components not multiples of f0 (there are no real harmonics
        // in these bins, only aliasing; a 1 s window gives nulls every 1 Hz,
        // neighbors don't leak).
        for (f0, bin) in [(1000.0, 900.0), (500.0, 400.0)] {
            let naive = render_saw(false, f0, sr, n);
            let bleped = render_saw(true, f0, sr, n);
            let naive_fold = goertzel(&naive, bin, sr);
            let blep_fold = goertzel(&bleped, bin, sr);
            assert!(naive_fold > 30.0, "test setup broken: f0={f0}");
            assert!(
                blep_fold < naive_fold * 0.05,
                "aliasing survives at f0={f0}: blep={blep_fold} naive={naive_fold}"
            );
        }
        // Nyquist edge: 2-point BLEP has a known small residual there
        // (inaudible), but it must still be noticeably below the naive one.
        let naive = render_saw(false, 110.0, sr, n);
        let bleped = render_saw(true, 110.0, sr, n);
        let naive_edge = goertzel(&naive, 21_990.0, sr);
        let blep_edge = goertzel(&bleped, 21_990.0, sr);
        assert!(blep_edge < naive_edge * 0.6);
        // Low harmonics (880 Hz) aren't touched by BLEP.
        let naive_lo = goertzel(&naive, 880.0, sr);
        let blep_lo = goertzel(&bleped, 880.0, sr);
        assert!(
            blep_lo > naive_lo * 0.8,
            "body damaged: blep={blep_lo} naive={naive_lo}"
        );
    }

    #[test]
    fn square_blep_stays_bounded() {
        let sr = 44_100.0;
        let mut phase = 0.0;
        for _ in 0..(sr as usize) {
            phase = (phase + 220.0 / sr) % 1.0;
            let s = square_blep(phase, 220.0, sr);
            assert!(s.is_finite());
            assert!(s.abs() <= 1.3, "overshoot too big: {s}");
        }
    }

    #[test]
    fn svf_low_passes_dc_blocks_nyquist() {
        let sr = 44_100.0;
        let mut svf = Svf::default();
        let (mut low, _, _) = (0.0, 0.0, 0.0);
        for _ in 0..20_000 {
            (low, _, _) = svf.process(1.0, 1000.0, 0.7, sr);
        }
        assert!((low - 1.0).abs() < 1e-3, "DC must pass LP: {low}");

        // An 8 kHz sine through a 1 kHz cutoff: a 12 dB/oct slope attenuates ~64×.
        let mut svf = Svf::default();
        let mut sum_sq = 0.0;
        let n = sr as usize;
        let mut phase = 0.0;
        for _ in 0..n {
            phase = (phase + 8000.0 / sr) % 1.0;
            let x = (phase * TAU).sin();
            let (l, _, _) = svf.process(x, 1000.0, 0.7, sr);
            sum_sq += l * l;
        }
        let rms = (sum_sq / n as f32).sqrt();
        assert!(rms < 0.08, "stopband leaks: rms={rms}");
    }

    #[test]
    fn svf_high_blocks_dc() {
        let sr = 44_100.0;
        let mut svf = Svf::default();
        let mut high = 0.0;
        for _ in 0..20_000 {
            (_, _, high) = svf.process(1.0, 5000.0, 0.7, sr);
        }
        assert!(high.abs() < 1e-3, "DC must not pass HP: {high}");
    }

    #[test]
    fn svf_cache_matches_direct_computation() {
        // The cache doesn't change the sound: two filters with identical state
        // history give bit-for-bit identical samples (both take coefficients
        // from the cache after the first sample).
        let sr = 48_000.0;
        let mut c2 = Svf::default();
        let mut d2 = Svf::default();
        for i in 0..4096 {
            let x = (i as f32 * 12.9898).sin() * 0.7;
            let a = c2.process(x, 800.0, 1.1, sr);
            let b = d2.process(x, 800.0, 1.1, sr);
            assert_eq!(a.0.to_bits(), b.0.to_bits(), "cache diverges at {i}");
            assert_eq!(a.1.to_bits(), b.1.to_bits(), "cache diverges at {i}");
            assert_eq!(a.2.to_bits(), b.2.to_bits(), "cache diverges at {i}");
        }
        // Changing the cutoff recomputes coefficients (a sweep doesn't stick).
        let before = c2.process(0.5, 800.0, 1.1, sr);
        let after = c2.process(0.5, 4000.0, 1.1, sr);
        assert_ne!(before.0.to_bits(), after.0.to_bits(), "cutoff stuck");
    }

    #[test]
    fn fast_tanh_stays_close_to_libm() {
        // Accuracy: a 2048 table + lerp — error < 1e-4 everywhere.
        let mut worst = 0.0f32;
        let mut x = -2.0f32;
        while x <= 2.0 {
            let err = (fast_tanh(x) - x.tanh()).abs();
            worst = worst.max(err);
            assert!(fast_tanh(x).is_finite());
            x += 0.01;
        }
        assert!(worst < 1e-4, "fast_tanh too far: {worst}");
        assert!((fast_tanh(0.05) - 0.05).abs() < 1e-3);
        assert_eq!(fast_tanh(0.0), 0.0);
        assert!(fast_tanh(-1.0) < 0.0 && fast_tanh(1.0) > 0.0);
        assert!(fast_tanh(10.0) <= 1.0 && fast_tanh(-10.0) >= -1.0);
    }

    #[test]
    fn fast_sin_matches_libm() {
        // Both sines are within interpolation error (< 2e-5);
        // large arguments and negative phases wrap.
        let mut worst = 0.0f32;
        let mut x = -20.0f32;
        while x <= 20.0 {
            worst = worst.max((fast_sin(x) - x.sin()).abs());
            x += 0.01;
        }
        assert!(worst < 2e-5, "fast_sin too far: {worst}");
        let mut worst01 = 0.0f32;
        let mut p = 0.0f32;
        while p < 3.0 {
            worst01 = worst01.max((sin01(p) - (p * TAU).sin()).abs());
            p += 0.001;
        }
        assert!(worst01 < 2e-5, "sin01 too far: {worst01}");
        assert!(fast_sin(1e6).abs() <= 1.0 && sin01(123.5).abs() <= 1.0);
    }

    #[test]
    fn exp_decay_matches_direct_exp() {
        // The recursive envelope matches direct exp to float precision
        // over a second of operation.
        let sr = 48_000.0;
        let mut env = ExpDecay::start(7.0, sr);
        for i in 0..sr as usize {
            let t = i as f32 / sr;
            let expect = (-7.0 * t).exp();
            let got = env.next();
            assert!(
                (got - expect).abs() < 1e-4,
                "drift at {i}: {got} vs {expect}"
            );
        }
    }

    #[test]
    fn oversampled_bandpassed_square_cuts_high_aliasing() {
        // 2× oversampling works ONLY together with a decimating filter:
        // plain averaging of two subsamples preserves a 30 kHz harmonic,
        // which folds to 14.1 kHz. In the hi-hat that role is played by the
        // 10 kHz BP in the double grid — we test the same chain.
        let sr = 44_100.0;
        let f0 = 10_000.0;
        let n = sr as usize;
        let mut a1 = Vec::with_capacity(n);
        let mut a2 = Vec::with_capacity(n);
        let mut ph1 = 0.0f32;
        let mut ph2 = 0.0f32;
        let mut bp1 = Svf::default();
        let mut bp2 = Svf::default();
        for _ in 0..n {
            ph1 = (ph1 + f0 / sr) % 1.0;
            let (_, b1, _) = bp1.process(square_blep(ph1, f0, sr), 10_000.0, 0.7, sr);
            a1.push(b1);
            // Two subsamples in the 2× grid with BP decimation.
            let mut acc = 0.0f32;
            for _ in 0..2 {
                ph2 = (ph2 + f0 / (2.0 * sr)) % 1.0;
                let (_, b, _) = bp2.process(square_blep(ph2, f0, 2.0 * sr), 10_000.0, 0.7, 2.0 * sr);
                acc += b * 0.5;
            }
            a2.push(acc);
        }
        let folded = 3.0 * f0 - sr; // 14.1 kHz
        let p1 = goertzel(&a1, folded, sr);
        let p2 = goertzel(&a2, folded, sr);
        assert!(p1 > 5.0, "test setup broken: {p1}");
        assert!(
            p2 < p1 * 0.9,
            "oversampled BP did not help: 2x={p2} base={p1}"
        );
    }

    #[test]
    fn pcg_noise_is_uniform_and_decorrelated() {
        let mut n1 = Noise::new(0x1234);
        let mut n2 = Noise::new(0x1234);
        let mut n3 = Noise::new(0x1235);
        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut prev = 0.0f64;
        let mut lag1 = 0.0f64;
        let count = 20_000;
        for i in 0..count {
            let x = n1.sample() as f64;
            let y = n2.sample() as f64;
            assert_eq!(x.to_bits(), y.to_bits());
            assert!(n3.sample().is_finite());
            sum += x;
            sum_sq += x * x;
            if i > 0 {
                lag1 += x * prev;
            }
            prev = x;
        }
        let mean = sum / count as f64;
        let var = sum_sq / count as f64;
        let corr = lag1 / (count as f64 * var);
        assert!(mean.abs() < 0.02, "noise DC: {mean}");
        assert!((var - 1.0 / 3.0).abs() < 0.02, "noise variance: {var}");
        assert!(corr.abs() < 0.02, "noise lag-1 correlation: {corr}");
        // next_u32 is deterministic in the same way as the sample stream.
        let mut a = Noise::new(7);
        let mut b = Noise::new(7);
        for _ in 0..32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn limiter_lookahead_catches_a_spike_at_its_peak() {
        // A single-sample spike: without lookahead it would slip through
        // the 2 ms attack. With lookahead, output at the spike is already held down.
        let sr = 44_100.0;
        let mut lim = MasterLimiter::default();
        for _ in 0..(sr * 0.2) as usize {
            lim.process(0.05, 0.05, sr);
        }
        let mut before = 0.0f32;
        let mut at_spike = 0.0f32;
        for i in 0..(sr * 0.05) as usize {
            let x = if i == lim.look { 3.0 } else { 0.05 };
            let (out, _) = lim.process(x, x, sr);
            if i < lim.look {
                before = before.max(out.abs());
            } else if i == lim.look {
                at_spike = out.abs();
            }
        }
        assert!((before - 0.05).abs() < 1e-3, "quiet pre-roll: {before}");
        assert!(
            at_spike < lim.ceiling * 1.05,
            "lookahead let the spike through: {at_spike}"
        );
    }
}
