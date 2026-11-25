//! Master space: a compact stereo reverb (light Freeverb)
//! plus an optional tempo-synced delay.
//!
//! The reverb is not tempo-locked (depth without rhythmic risk),
//! the delay is a dotted 8th of the live BPM.
//! The kick is not sent (highpass): the lows stay dry and readable.
//! All structs are concrete, with no generics — safe for the FFI parser.

use crate::instruments::dsp::sin01;

const COMB_TUNINGS_L: [f32; 4] = [1116.0, 1188.0, 1277.0, 1356.0];
const COMB_TUNINGS_R: [f32; 4] = [1139.0, 1211.0, 1300.0, 1379.0];
const AP_TUNINGS_L: [f32; 2] = [556.0, 441.0];
const AP_TUNINGS_R: [f32; 2] = [579.0, 464.0];
const AP_FEEDBACK: f32 = 0.5;
/// Send highpass: nothing below 650 Hz goes into the reverb.
const SEND_HP_HZ: f32 = 650.0;
/// Predelay: 20 ms before the combs — so the direct sound and early reflections
/// don't merge, the room reads as space rather than as a chorus.
const PREDELAY_SECS: f32 = 0.020;
const PREDELAY_CAP: usize = 2048;
/// Cross-feedback between banks: the left comb hears the right
/// tail and vice versa. Without this, two independent mono banks gave
/// "two monos" instead of one stereo space.
const CROSS_FEEDBACK: f32 = 0.16;
/// Allpass modulation: it jitters the fixed delays, killing the
/// metallic ringing of static combs.
const AP_LFO_HZ: f32 = 0.32;
const AP_LFO_DEPTH: f32 = 0.38;
/// Anti-denormal floor: reverb lines live forever and without this drift
/// into the subnormal range, where x86 arithmetic is dozens of times slower.
const FEED_GUARD: f32 = 1e-18;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn with_len(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            idx: 0,
            store: 0.0,
        }
    }

    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn with_len(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            idx: 0,
        }
    }

    /// `frac` — fractional addition to the delay [0,1). Interpolation between
    /// adjacent samples of the line: the delay breathes without a step click.
    fn process(&mut self, x: f32, frac: f32) -> f32 {
        let n = self.buf.len();
        let d0 = self.buf[self.idx];
        let d1 = self.buf[(self.idx + 1) % n];
        let delayed = d0 + (d1 - d0) * frac;
        let out = -x + delayed;
        self.buf[self.idx] = x + delayed * AP_FEEDBACK;
        self.idx = (self.idx + 1) % n;
        out
    }
}

pub struct SpaceBus {
    combs_l: [Comb; 4],
    combs_r: [Comb; 4],
    ap_l: [Allpass; 2],
    ap_r: [Allpass; 2],
    /// Predelay line: one per channel.
    pre_l: [f32; PREDELAY_CAP],
    pre_r: [f32; PREDELAY_CAP],
    pre_write: usize,
    pre_delay: usize,
    /// Sample period — needed for the allpass LFO.
    sample_dt: f32,
    /// One sample of cross-feedback (loop delay = 1 sample).
    cross_l: f32,
    cross_r: f32,
    ap_lfo: f32,
    hp_a: f32,
    hp_y_l: f32,
    hp_y_r: f32,
    hp_x_l: f32,
    hp_x_r: f32,
    wet: f32,
    feedback: f32,
    damp: f32,
}

impl SpaceBus {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let comb_len = |t: f32| (t * scale).round() as usize;
        let ap_len = |t: f32| (t * scale).round() as usize;
        let mut bus = Self {
            combs_l: std::array::from_fn(|i| Comb::with_len(comb_len(COMB_TUNINGS_L[i]))),
            combs_r: std::array::from_fn(|i| Comb::with_len(comb_len(COMB_TUNINGS_R[i]))),
            ap_l: std::array::from_fn(|i| Allpass::with_len(ap_len(AP_TUNINGS_L[i]))),
            ap_r: std::array::from_fn(|i| Allpass::with_len(ap_len(AP_TUNINGS_R[i]))),
            pre_l: [0.0; PREDELAY_CAP],
            pre_r: [0.0; PREDELAY_CAP],
            pre_write: 0,
            pre_delay: 0,
            sample_dt: 1.0 / 44_100.0,
            cross_l: 0.0,
            cross_r: 0.0,
            ap_lfo: 0.0,
            hp_a: 0.0,
            hp_y_l: 0.0,
            hp_y_r: 0.0,
            hp_x_l: 0.0,
            hp_x_r: 0.0,
            wet: 0.16,
            feedback: 0.82,
            damp: 0.45,
        };
        bus.retune(sample_rate);
        bus
    }

    /// Sample-rate change WITHOUT losing the tail: the delay lines
    /// stay in place (their length in samples changes by <10% between
    /// 44.1 and 48 kHz — audibly the same tail); only predelay and
    /// the send-HP coefficient are recalculated.
    pub fn retune(&mut self, sample_rate: f32) {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        let rc = 1.0 / (std::f32::consts::TAU * SEND_HP_HZ);
        let dt = 1.0 / sr;
        self.hp_a = rc / (rc + dt);
        self.pre_delay = ((sr * PREDELAY_SECS) as usize)
            .clamp(1, PREDELAY_CAP - 1);
        self.sample_dt = 1.0 / sr;
    }

    /// Room size from style: wet 0..0.8, size 0..1 (room → cathedral).
    /// A bigger room is longer and darker.
    pub fn set_room(&mut self, wet: f32, size: f32) {
        let size = size.clamp(0.0, 1.0);
        self.wet = wet.clamp(0.0, 0.8);
        self.feedback = 0.62 + 0.20 * size;
        self.damp = 0.45 + 0.25 * size;
    }

    fn send_hp(&mut self, x: f32, left: bool) -> f32 {
        let (y_prev, x_prev) = if left {
            (self.hp_y_l, self.hp_x_l)
        } else {
            (self.hp_y_r, self.hp_x_r)
        };
        let mut y = self.hp_a * (y_prev + x - x_prev);
        if y.abs() < 1e-15 {
            y = 0.0;
        }
        if left {
            self.hp_y_l = y;
            self.hp_x_l = x;
        } else {
            self.hp_y_r = y;
            self.hp_x_r = x;
        }
        y
    }

    /// Full reset of the lines (guard against a stuck NaN — a feedback
    /// reverb would otherwise ring forever). Also mutes the room tail
    /// on a seed or style change: old generation doesn't keep playing.
    pub fn reset(&mut self) {
        self.reset_all();
    }

    fn reset_all(&mut self) {
        for comb in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            comb.buf.fill(0.0);
            comb.store = 0.0;
        }
        for ap in self.ap_l.iter_mut().chain(self.ap_r.iter_mut()) {
            ap.buf.fill(0.0);
        }
        self.pre_l.fill(0.0);
        self.pre_r.fill(0.0);
        self.pre_write = 0;
        self.cross_l = 0.0;
        self.cross_r = 0.0;
        self.hp_y_l = 0.0;
        self.hp_y_r = 0.0;
        self.hp_x_l = 0.0;
        self.hp_x_r = 0.0;
    }

    /// Stereo input → dry + wet tail with predelay and cross-links.
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // Predelay: write now, read `pre_delay` back.
        self.pre_l[self.pre_write] = l;
        self.pre_r[self.pre_write] = r;
        let pre_read = (self.pre_write + PREDELAY_CAP - self.pre_delay) % PREDELAY_CAP;
        let delayed_l = self.pre_l[pre_read];
        let delayed_r = self.pre_r[pre_read];
        self.pre_write = (self.pre_write + 1) % PREDELAY_CAP;

        let (fb, damp, wet) = (self.feedback, self.damp, self.wet);
        let send_l = self.send_hp(delayed_l, true) + FEED_GUARD + self.cross_r * CROSS_FEEDBACK;
        let send_r = self.send_hp(delayed_r, false) + FEED_GUARD + self.cross_l * CROSS_FEEDBACK;

        let mut wet_l = 0.0;
        for comb in &mut self.combs_l {
            wet_l += comb.process(send_l, fb, damp);
        }
        wet_l *= 0.25;
        let mut wet_r = 0.0;
        for comb in &mut self.combs_r {
            wet_r += comb.process(send_r, fb, damp);
        }
        wet_r *= 0.25;

        // Modulate the allpass delay: L and R are 90° out of phase,
        // so the tail spreads across the sides and doesn't ring predictably.
        self.ap_lfo = (self.ap_lfo + AP_LFO_HZ * self.sample_dt) % 1.0;
        let frac_l = 0.5 + AP_LFO_DEPTH * sin01(self.ap_lfo);
        let frac_r = 0.5 + AP_LFO_DEPTH * sin01(self.ap_lfo + 0.25);
        for ap in &mut self.ap_l {
            wet_l = ap.process(wet_l, frac_l);
        }
        for ap in &mut self.ap_r {
            wet_r = ap.process(wet_r, frac_r);
        }
        self.cross_l = wet_l;
        self.cross_r = wet_r;

        // The dry signal is NOT delayed: predelay applies only to the send
        // into the reverb, otherwise instrument attacks shift by 20 ms.
        let (mut out_l, mut out_r) = (l + wet_l * wet, r + wet_r * wet);
        if !(out_l.is_finite() && out_r.is_finite()) {
            self.reset_all();
            out_l = 0.0;
            out_r = 0.0;
        }
        (out_l, out_r)
    }
}

/// Tempo-synced feedback delay (dotted 8th): lead echoes in the grid.
/// The time is set by the engine from the live BPM; one line per channel.
///
/// DISABLED in the pipeline (see RenderState): situational, smears transients
/// and drags the previous section's tail into the new one. It sits here as a
/// ready-made standalone feature: enabling it again = send from the mixer + set_time from the
/// live tempo, rather than hardcoded divisions by role.
pub struct Echo {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_idx: usize,
    read_offset: usize,
    feedback: f32,
    lp_a: f32,
    lp_y_l: f32,
    lp_y_r: f32,
}

impl Echo {
    /// `max_secs` — line headroom (enough for the slowest tempo).
    pub fn new(sample_rate: f32, max_secs: f32, feedback: f32, damp_hz: f32) -> Self {
        let len = ((max_secs * sample_rate) as usize).max(64);
        let rc = 1.0 / (std::f32::consts::TAU * damp_hz);
        let dt = 1.0 / sample_rate;
        let mut echo = Self {
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            write_idx: 0,
            read_offset: len / 2,
            feedback: feedback.clamp(0.0, 0.9),
            lp_a: rc / (rc + dt),
            lp_y_l: 0.0,
            lp_y_r: 0.0,
        };
        echo.set_time(0.32, sample_rate);
        echo
    }

    pub fn set_time(&mut self, secs: f32, sample_rate: f32) {
        let len = self.buf_l.len();
        self.read_offset = ((secs * sample_rate) as usize).clamp(16, len - 1);
    }

    /// Send stereo → wet repeats. No dry signal in the output.
    pub fn process(&mut self, send_l: f32, send_r: f32) -> (f32, f32) {
        let len = self.buf_l.len();
        let read = (self.write_idx + len - self.read_offset) % len;
        let mut wet_l = self.buf_l[read];
        let mut wet_r = self.buf_r[read];
        if wet_l.abs() < 1e-15 {
            wet_l = 0.0;
        }
        if wet_r.abs() < 1e-15 {
            wet_r = 0.0;
        }
        // Damping in the loop: repeats get darker, as with analog gear.
        self.lp_y_l += self.lp_a * (wet_l - self.lp_y_l);
        self.lp_y_r += self.lp_a * (wet_r - self.lp_y_r);
        self.buf_l[self.write_idx] = send_l + self.lp_y_l * self.feedback;
        self.buf_r[self.write_idx] = send_r + self.lp_y_r * self.feedback;
        self.write_idx = (self.write_idx + 1) % len;
        if !(wet_l.is_finite() && wet_r.is_finite()) {
            self.buf_l.fill(0.0);
            self.buf_r.fill(0.0);
            return (0.0, 0.0);
        }
        (wet_l, wet_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_in_silence_out() {
        let mut space = SpaceBus::new(44_100.0);
        for _ in 0..1000 {
            let (l, r) = space.process(0.0, 0.0);
            assert!(l.abs() < 1e-9 && r.abs() < 1e-9);
        }
    }

    #[test]
    fn nan_input_recovers_to_silence() {
        let mut space = SpaceBus::new(44_100.0);
        let (l, r) = space.process(f32::NAN, 0.0);
        assert!(l.is_finite() && r.is_finite());
        let (l, r) = space.process(0.5, 0.5);
        assert!(l.is_finite() && r.is_finite());
    }

    #[test]
    fn impulse_rings_and_decays() {
        let sr = 44_100.0;
        let mut space = SpaceBus::new(sr);
        let (l0, r0) = space.process(1.0, 1.0);
        assert!(l0.is_finite() && r0.is_finite());
        let mut first = 0.0;
        let mut second = 0.0;
        for i in 0..(sr as usize) {
            let (l, r) = space.process(0.0, 0.0);
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() < 2.0 && r.abs() < 2.0);
            let e = l * l + r * r;
            if i < (0.5 * sr) as usize {
                first += e;
            } else {
                second += e;
            }
        }
        assert!(
            first > second * 4.0,
            "reverb does not decay: {first} vs {second}"
        );
    }

    #[test]
    fn room_size_changes_tail() {
        let sr = 44_100.0;
        let late_energy = |size: f32| {
            let mut space = SpaceBus::new(sr);
            space.set_room(0.5, size);
            space.process(1.0, 1.0);
            let mut energy = 0.0;
            for i in 0..(sr as usize) {
                let (l, r) = space.process(0.0, 0.0);
                assert!(l.is_finite() && r.is_finite());
                if i >= (0.5 * sr) as usize {
                    energy += l * l + r * r;
                }
            }
            energy
        };
        let room = late_energy(0.2);
        let cathedral = late_energy(0.9);
        assert!(
            cathedral > room * 5.0,
            "room size does nothing: {cathedral} vs {room}"
        );
    }

    #[test]
    fn stereo_banks_are_coupled() {
        // An impulse in the left channel alone must produce energy in the right
        // (cross-feedback) — otherwise these are two independent mono reverbs.
        let sr = 44_100.0;
        let mut space = SpaceBus::new(sr);
        space.set_room(0.5, 0.5);
        space.process(1.0, 0.0);
        let mut right = 0.0;
        for _ in 0..(0.5 * sr) as usize {
            let (_, r) = space.process(0.0, 0.0);
            right += r * r;
        }
        assert!(right > 1e-4, "right bank stays silent: {right}");
    }

    #[test]
    fn retune_keeps_the_tail() {
        // Changing the output rate doesn't cut the tail: after retune the energy
        // keeps flowing instead of dropping to zero.
        let mut space = SpaceBus::new(44_100.0);
        space.process(1.0, 1.0);
        let mut before = 0.0;
        for _ in 0..1000 {
            let (l, r) = space.process(0.0, 0.0);
            before += l * l + r * r;
        }
        space.retune(48_000.0);
        let mut after = 0.0;
        for _ in 0..1000 {
            let (l, r) = space.process(0.0, 0.0);
            after += l * l + r * r;
        }
        assert!(after > before * 0.1, "retune cut the tail: {after} vs {before}");
    }

    #[test]
    fn echo_repeats_then_dies() {
        let sr = 44_100.0;
        let mut echo = Echo::new(sr, 1.0, 0.38, 2800.0);
        echo.set_time(0.2, sr);
        let (w0, _) = echo.process(1.0, 1.0);
        assert!(w0.is_finite());
        let mut heard: f32 = 0.0;
        for i in 0..((0.5 * sr) as usize) {
            let (l, r) = echo.process(0.0, 0.0);
            assert!(l.is_finite() && r.is_finite());
            if i > ((0.19 * sr) as usize) && i < ((0.25 * sr) as usize) {
                heard = heard.max(l.abs().max(r.abs()));
            }
        }
        assert!(heard > 0.05, "no echo repeat: {heard}");
        for _ in 0..((3.0 * sr) as usize) {
            echo.process(0.0, 0.0);
        }
        let (l, r) = echo.process(0.0, 0.0);
        assert!(l.abs() < 1e-3 && r.abs() < 1e-3);
    }
}
