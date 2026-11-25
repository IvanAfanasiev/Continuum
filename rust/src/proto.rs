//! Prototype of the pure-formula engine: all music is closed-form functions
//! of time `y(seed, t)`. No scheduler, notes or voices:
//! kick, bass, hats, snare and lead read the same clock, so they
//! cannot drift apart by definition. State exists only where it
//! cannot be avoided (filters), and it lives inside the structure —
//! the same call sequence yields the same samples.
//!
//! It lives ALONGSIDE the main engine for a fair A/B, not instead of it.
//! CLI: `cargo run -- --proto 7 --wav proto.wav --secs 30`.

use std::f32::consts::TAU;

use crate::arranger::splitmix64;

/// Prototype tempo: club 126, like the main engine (fair A/B).
pub const PROTO_BPM: f32 = 126.0;
/// Scale: natural minor from the seed's tonic.
const SCALE: [i8; 7] = [0, 2, 3, 5, 7, 8, 10];
/// Root progression (degrees): the tonic holds, the others roam.
const PROG_LEN: usize = 4;
/// Lead skeleton on 16th notes: anchors on the beats, life in between.
const LEAD_W: [f32; 16] = [
    1.0, 0.0, 0.4, 0.0, 0.6, 0.0, 0.4, 0.0, 0.7, 0.0, 0.4, 0.0, 0.5, 0.0, 0.4, 0.2,
];

/// Deterministic hash → [0, 1). All the "randomness" of the prototype.
fn hash01(a: u64, b: u64) -> f32 {
    ((splitmix64(a ^ b.wrapping_mul(0x9E3779B9)) >> 32) as f32) / (u32::MAX as f32)
}

/// Track parameters from the seed: tonic, motif, progression, swing, density.
#[derive(Clone, Copy)]
struct ProtoParams {
    tonic: u8,
    motif: [u8; 16],
    prog: [u8; PROG_LEN],
    swing: f32,
    density: f32,
}

impl ProtoParams {
    fn new(seed: u64) -> Self {
        let h = splitmix64(seed ^ 0x90E7);
        let mut motif = [0u8; 16];
        let mut m = h ^ 0x9071F;
        for slot in motif.iter_mut() {
            m = splitmix64(m);
            *slot = (m % 5) as u8;
        }
        let mut prog = [0u8; PROG_LEN];
        let mut p = h ^ 0x9206;
        for slot in prog.iter_mut() {
            p = splitmix64(p);
            *slot = (p % 5) as u8;
        }
        Self {
            tonic: 33 + (h % 6) as u8,
            motif,
            prog,
            swing: 0.93 + 0.14 * ((splitmix64(h ^ 0x5716) >> 32) as f32 / u32::MAX as f32),
            density: 0.7 + 0.6 * ((splitmix64(h ^ 0xD517) >> 32) as f32 / u32::MAX as f32),
        }
    }
}

/// Position in the form from the bar: 8 bars in a loop —
/// 2 intro, 2 build-up, 2 drop, 2 outro. Returns layer gains
/// (kick, low, high, lead): the kick always holds, the others breathe.
fn form_gains(bar8: usize) -> (f32, f32, f32, f32) {
    match bar8 {
        0 | 1 => (0.7, 0.0, 0.4, 0.0),
        2 | 3 => (0.9, 0.5, 0.7, 0.4),
        4 | 5 => (1.0, 1.0, 1.0, 1.0),
        _ => (0.9, 0.8, 0.8, 0.7),
    }
}

fn midi_to_freq(m: u8) -> f32 {
    440.0 * 2f32.powf((m as f32 - 69.0) / 12.0)
}

/// Prototype engine: renders blocks sequentially.
/// State (`n`, filters) — continuity between blocks.
pub struct ProtoEngine {
    params: ProtoParams,
    seed: u64,
    sample_rate: f32,
    n: u64,
    bass_lp: f32,
    dc_x: f32,
    dc_y: f32,
    /// Smoothed form gains: sections switch without steps.
    gains: [f32; 4],
}

impl ProtoEngine {
    pub fn new(seed: u64, sample_rate: f32) -> Self {
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            44_100.0
        };
        Self {
            params: ProtoParams::new(seed),
            seed,
            sample_rate: sr,
            n: 0,
            bass_lp: 0.0,
            dc_x: 0.0,
            dc_y: 0.0,
            gains: [0.0; 4],
        }
    }

    fn beat16(&self) -> f32 {
        60.0 / PROTO_BPM / 4.0
    }

    /// Grid step with the seed's swing: odd 16ths are late.
    fn step_at(&self, t: f32) -> (usize, f32) {
        let b = self.beat16();
        let raw = t / b;
        let i = raw.floor() as usize;
        let frac = raw - i as f32;
        // Swing: an odd step starts later by (swing-1)*subdivision.
        let shift = if i % 2 == 1 {
            (self.params.swing - 1.0) * 2.0
        } else {
            0.0
        };
        (i, frac - shift.min(0.9))
    }

    fn kick(&self, step: usize, local: f32) -> f32 {
        if !step.is_multiple_of(4) || local < 0.0 {
            return 0.0;
        }
        let d = local * self.beat16();
        // Punch from the video: body exponent + downward sweep + attack click.
        let env = (-9.0 * d).exp();
        // Analytical phase of the 160→45 Hz sweep: no discontinuities by construction.
        let phase = 160.0 * (1.0 - (-30.0 * d).exp()) / 30.0 + 45.0 * d;
        let body = (TAU * phase).sin() * env;
        let click = (-300.0 * d).exp() * 0.4;
        // Video offset (−2.2): a gritty tail instead of a clean decay.
        ((body * 4.7 - 2.2 * env) * 0.35 + click).tanh() * 0.8
    }

    fn bass(&mut self, t: f32, step: usize, gain: f32) -> f32 {
        if gain <= 0.0 {
            return 0.0;
        }
        let bar = step / 16;
        let root_deg = self.params.prog[(bar / 2) % PROG_LEN] as usize;
        let midi = self.params.tonic + SCALE[root_deg % 7] as u8;
        let freq = midi_to_freq(midi);
        let x = t * freq;
        // Video's bit formula, DC removed, through a soft LP.
        let m1 = (x.floor() as i64 & 1) as f32;
        let m2 = ((x * 2.0).floor() as i64 & 1) as f32;
        let m4 = ((x * 4.0).floor() as i64 & 1) as f32;
        let raw = (-1.0 + 0.25 * m2 + m4 * m1 + 0.625) * 0.7;
        self.bass_lp += 0.12 * (raw - self.bass_lp);
        self.bass_lp * gain * 0.5
    }

    fn hats(&self, step: usize, local: f32, gain: f32) -> f32 {
        if gain <= 0.0 || local < 0.0 {
            return 0.0;
        }
        // Offbeat 8ths + 16th ghosts: deterministic noise tick.
        let odd8 = step % 4 == 2;
        let ghost = step % 2 == 1 && hash01(self.seed, step as u64) < 0.3 * self.params.density;
        if !odd8 && !ghost {
            return 0.0;
        }
        let d = local * self.beat16();
        let env = (-60.0 * d).exp();
        let idx = self.n;
        let noise = hash01(self.seed ^ 0x8A7, idx) * 2.0 - 1.0;
        noise * env * gain * if odd8 { 0.20 } else { 0.10 }
    }

    fn snare(&self, step: usize, local: f32, gain: f32) -> f32 {
        // Half-time: only the 3rd beat (step 8), like a funk reference.
        if step % 16 != 8 || local < 0.0 || gain <= 0.0 {
            return 0.0;
        }
        let d = local * self.beat16();
        let body = (TAU * 190.0 * d).sin() * (-25.0 * d).exp();
        let noise = hash01(self.seed ^ 0x5A2, self.n) * 2.0 - 1.0;
        (body * 0.7 + noise * (-40.0 * d).exp() * 0.5) * gain * 0.5
    }

    fn lead(&self, step: usize, local: f32, gain: f32) -> f32 {
        if gain <= 0.0 || local < 0.0 {
            return 0.0;
        }
        let sim = step % 16;
        let w = LEAD_W[sim];
        if w <= 0.0 || hash01(self.seed ^ 0x1EAD, step as u64) >= w * self.params.density {
            return 0.0;
        }
        // Seed motif a third up on odd phrases (call and response).
        let phrase = step / 32;
        let mut deg = self.params.motif[(step / 2) % 16] as usize;
        if phrase % 2 == 1 {
            deg += 2;
        }
        let midi = self.params.tonic + 24 + SCALE[deg % 7] as u8;
        let freq = midi_to_freq(midi);
        let x = local * self.beat16() * freq + step as f32 * 0.5;
        // Funk stack in just intonation: unison + fifth + octave.
        let tone =
            (TAU * x).sin() * 0.55 + (TAU * x * 1.5).sin() * 0.25 + (TAU * x * 2.0).sin() * 0.2;
        let d = local * self.beat16();
        // Step edges are faded: the gate switches without a click.
        let edge = (local / 0.05).min(1.0) * ((1.0 - local) / 0.05).min(1.0);
        tone * (-4.0 * d).exp().min(1.0) * edge * gain * 0.4
    }

    /// One stereo sample (mono doubled): sum of layers through the form and tanh.
    fn sample(&mut self) -> (f32, f32) {
        let t = self.n as f32 / self.sample_rate;
        let (step, local) = self.step_at(t);
        let bar8 = (step / 16) % 8;
        // Form without steps: gains chase their targets (~8/s).
        let (tk, tb, th, tl) = form_gains(bar8);
        let c = 1.0 - (-8.0 / self.sample_rate).exp();
        for (g, t) in self.gains.iter_mut().zip([tk, tb, th, tl]) {
            *g += (t - *g) * c;
        }
        let (g_k, g_b, g_h, g_l) = (self.gains[0], self.gains[1], self.gains[2], self.gains[3]);
        let k = self.kick(step, local);
        let b = self.bass(t, step, g_b);
        let h = self.hats(step, local, g_h);
        let s = self.snare(step, local, g_h);
        let l = self.lead(step, local, g_l);
        let m = (k * g_k + b + h + s + l).tanh() * 0.8;
        // DC blocker: the bit formula without LP would produce DC.
        let y = m - self.dc_x + 0.995 * self.dc_y;
        self.dc_x = m;
        self.dc_y = y;
        self.n += 1;
        (y, y)
    }

    /// Block of stereo interleaved f32. Sequential calls —
    /// a continuous stream (state lives inside).
    pub fn render_block(&mut self, frames: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            let (l, r) = self.sample();
            out.push(l);
            out.push(r);
        }
        out
    }
}

/// Prototype run without recording: peak/RMS/speed for A/B with soak.
pub struct ProtoSoak {
    pub peak: f32,
    pub rms: f32,
    pub non_finite: u64,
    pub frames: u64,
    pub wall_secs: f32,
}

pub fn proto_soak(seed: u64, seconds: f32, sample_rate: f32) -> ProtoSoak {
    let wall = std::time::Instant::now();
    let mut eng = ProtoEngine::new(seed, sample_rate);
    let target = (seconds.max(0.0) * eng.sample_rate) as u64;
    let mut peak: f32 = 0.0;
    let mut sum_sq = 0.0;
    let mut non_finite = 0u64;
    let mut done = 0u64;
    while done < target {
        let block = eng.render_block(4096.min((target - done) as usize));
        for s in &block {
            if !s.is_finite() {
                non_finite += 1;
            } else {
                peak = peak.max(s.abs());
                sum_sq += s * s;
            }
        }
        done += block.len() as u64 / 2;
    }
    let frames = done;
    ProtoSoak {
        peak,
        rms: (sum_sq / (frames.max(1) * 2) as f32).sqrt(),
        non_finite,
        frames,
        wall_secs: wall.elapsed().as_secs_f32(),
    }
}

/// WAV export of the prototype (PCM16 stereo, header written by hand —
/// like the main pipeline, with no external dependencies).
pub fn proto_wav(
    seed: u64,
    seconds: f32,
    sample_rate: f32,
    path: &str,
) -> Result<ProtoSoak, String> {
    let mut eng = ProtoEngine::new(seed, sample_rate);
    let sr = eng.sample_rate;
    let target = (seconds.max(0.0) * sr) as u64;
    let mut pcm: Vec<i16> = Vec::with_capacity((target * 2) as usize);
    let mut peak: f32 = 0.0;
    let mut sum_sq = 0.0;
    let mut non_finite = 0u64;
    let mut done = 0u64;
    let wall = std::time::Instant::now();
    while done < target {
        for s in eng.render_block(4096.min((target - done) as usize)) {
            if !s.is_finite() {
                non_finite += 1;
            } else {
                peak = peak.max(s.abs());
                sum_sq += s * s;
                pcm.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
            }
            // pcm stereo interleaved is already in pairs — frame counter below.
        }
        done = pcm.len() as u64 / 2;
    }
    write_wav(&pcm, sr as u32, path)?;
    let frames = done;
    Ok(ProtoSoak {
        peak,
        rms: (sum_sq / (frames.max(1) * 2) as f32).sqrt(),
        non_finite,
        frames,
        wall_secs: wall.elapsed().as_secs_f32(),
    })
}

fn write_wav(pcm: &[i16], sample_rate: u32, path: &str) -> Result<(), String> {
    use std::io::Write;
    let data_bytes = pcm.len() * 2;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&2u16.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(sample_rate * 4).to_le_bytes());
    h[32..34].copy_from_slice(&4u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&(data_bytes as u32).to_le_bytes());
    let mut f = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
    f.write_all(&h).map_err(|e| format!("{path}: {e}"))?;
    for s in pcm {
        f.write_all(&s.to_le_bytes())
            .map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_blocks() {
        // Determinism: two engines — bitwise identical blocks.
        let (mut a, mut b) = (ProtoEngine::new(7, 44_100.0), ProtoEngine::new(7, 44_100.0));
        for _ in 0..4 {
            let (x, y) = (a.render_block(4096), b.render_block(4096));
            assert_eq!(x.len(), y.len());
            for (s, t) in x.iter().zip(y.iter()) {
                assert_eq!(s.to_bits(), t.to_bits());
            }
        }
    }

    #[test]
    fn sequential_blocks_join_cleanly() {
        // Continuity: 4×1024 == 1×4096 and no clicks at the seams.
        let mut a = ProtoEngine::new(7, 44_100.0);
        let mut seq = Vec::new();
        for _ in 0..4 {
            seq.extend(a.render_block(1024));
        }
        let mut b = ProtoEngine::new(7, 44_100.0);
        let one = b.render_block(4096);
        assert_eq!(seq.len(), one.len());
        let mut max_step = 0.0f32;
        for (i, (s, t)) in seq.iter().zip(one.iter()).enumerate() {
            assert_eq!(s.to_bits(), t.to_bits(), "drift at {i}");
            if i > 0 {
                max_step = max_step.max((s - seq[i - 1]).abs());
            }
        }
        assert!(max_step < 0.4, "clicks: {max_step}");
    }

    #[test]
    fn audible_finite_without_dc() {
        // Audibly, of course, without DC (mean ~0 over 5 seconds).
        let mut eng = ProtoEngine::new(11, 44_100.0);
        let out = eng.render_block(220_500);
        let mut peak = 0.0f32;
        let mut sum = 0.0f64;
        for s in &out {
            assert!(s.is_finite());
            peak = peak.max(s.abs());
            sum += *s as f64;
        }
        assert!(peak > 0.15, "too quiet: {peak}");
        assert!(peak <= 1.0, "clips: {peak}");
        let mean = sum / out.len() as f64;
        assert!(mean.abs() < 0.02, "DC leaks: {mean}");
    }

    #[test]
    fn drop_hits_harder_than_intro() {
        // The form works: the drop (bars 4–5) is denser than the intro (bars 0–1).
        let energy = |bars: std::ops::Range<usize>| {
            let mut eng = ProtoEngine::new(7, 44_100.0);
            // Rewind to the start of the range: cheaper to recreate and discard.
            let beat = 60.0 / PROTO_BPM;
            let skip = (bars.start as f32 * 4.0 * beat * 44_100.0) as usize;
            eng.render_block(skip);
            let out = eng.render_block((2.0 * 4.0 * beat * 44_100.0) as usize);
            out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32
        };
        let intro = energy(0..2);
        let drop = energy(4..6);
        assert!(drop > intro * 1.5, "no form: intro={intro} drop={drop}");
    }

    #[test]
    fn seeds_differ() {
        // Different seeds — different music: comparison in the drop zone,
        // where all layers play (the intro is similar for all — mostly kick).
        let e = |seed: u64| {
            let mut eng = ProtoEngine::new(seed, 44_100.0);
            eng.render_block(8 * 44_100);
            let out = eng.render_block(4 * 44_100);
            out.iter().map(|s| s.abs()).sum::<f32>() / out.len() as f32
        };
        let (a, b) = (e(7), e(424242));
        assert!((a - b).abs() > 1e-4, "seeds coincide: {a} vs {b}");
    }
}
