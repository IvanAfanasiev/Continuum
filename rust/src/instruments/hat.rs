use super::{
    dsp::{note_seed, release_fade, square_blep, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};

/// 808-style hi-hat: six inharmonic BLEP squares at the CLASSIC 808 cymbal
/// frequencies (205/304/370/523/540/800 Hz) → 10 kHz bandpass → 7 kHz highpass.
/// Openness is controlled by note duration, monophonic with choke like a live
/// hi-hat pedal.
///
/// Previous square waves at 7–13.6 kHz squealed at Nyquist: at 13.6 kHz
/// `freq/sr ≈ 0.31`, the BLEP window degenerates, and all the "metal" sat in
/// the last octave — a harsh whistle instead of a cymbal. Now the low-frequency
/// generators run at 2× oversampling: the upper octave forms a real 808-metal
/// spectrum, and the BP carves out the needed "iron" from it.
const OSC_HZ: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
/// Cymbal band: select 7–14 kHz from the harmonics of the six squares.
const METAL_BP_HZ: f32 = 10_000.0;
const METAL_BP_Q: f32 = 0.7;
/// Highpass cutoff: the hi-hat body lives above 7 kHz, leave the kick alone.
const HP_HZ: f32 = 7_000.0;
const HP_Q: f32 = 0.7;
const CLOSED_LEN_SECS: f32 = 0.05;
const OPEN_THRESHOLD_SECS: f32 = 0.12;
const MAX_OPEN_SECS: f32 = 0.5;
const CLOSED_DECAY: f32 = 90.0;
const OPEN_DECAY: f32 = 9.0;
/// Attack softened (1.5 ms): a transient shaper against sharp peaks,
/// as in the percussion spec. It was 0.5 ms — used to click.
const ATTACK_SECS: f32 = 0.0015;
const FADE_SECS: f32 = 0.006;
/// Noise layer on top of the BLEP metal: a live hi-hat is cymbal noise;
/// squares alone sound sterile (cf. v1: noise 0.65 + ring 0.35).
/// Kept quiet so it doesn't hiss: air, not sand.
const NOISE_GAIN: f32 = 0.2;
/// BP+HP take ~26 dB off the original signal: compensate so the tick's
/// loudness stays put (regression "hi-hat inaudible").
const METAL_GAIN: f32 = 2.4;

#[derive(Clone, Default)]
struct HatVoice {
    sample_rate: f32,
    phases: [f32; 6],
    t: f32,
    decay: f32,
    gain: f32,
    samples_left: usize,
    /// BP in the 2× grid: shapes the 808 metal before downsampling.
    metal_bp: Svf,
    hp: Svf,
    /// Recursive envelope instead of per-sample `exp`.
    env: ExpDecay,
    /// Cymbal noise: reseeded by note (identical ticks are identical).
    noise: Noise,
}

impl Voice for HatVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let open = note.duration_secs > OPEN_THRESHOLD_SECS;
        let len = if open {
            note.duration_secs.min(MAX_OPEN_SECS)
        } else {
            CLOSED_LEN_SECS
        };
        self.samples_left = (len * self.sample_rate).max(1.0) as usize;
        self.decay = if open { OPEN_DECAY } else { CLOSED_DECAY };
        self.gain = note.velocity;
        self.t = 0.0;
        self.env = ExpDecay::start(self.decay, self.sample_rate);
        self.metal_bp.reset();
        self.hp.reset();
        // Scatter oscillator phases from the note: metal lives on beatings
        // (identical phases every hit would give machine-gun), the choke click
        // pushes down the attack-ramp, and the previous hit's tail is the Mono crossfade.
        let seed = note_seed(note, 0x1A7);
        for (i, phase) in self.phases.iter_mut().enumerate() {
            let bits = seed.wrapping_add((i as u32).wrapping_mul(0x9E3779B9));
            *phase = (bits % 1000) as f32 / 1000.0;
        }
        self.noise = Noise::new(seed ^ 0x9E51);
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let double_sr = sample_rate * 2.0;
        // 2× oversampling: the metal filter works in the upper grid, where it
        // has room, and the two subsamples are averaged on downsampling
        // (simple 2-tap decimation — also suppresses the 22 kHz mirror).
        let mut metal = 0.0f32;
        for _ in 0..2 {
            let mut m = 0.0f32;
            for (phase, hz) in self.phases.iter_mut().zip(OSC_HZ) {
                *phase = (*phase + hz / double_sr) % 1.0;
                m += square_blep(*phase, hz, double_sr);
            }
            m /= OSC_HZ.len() as f32;
            let (_, bp, _) = self
                .metal_bp
                .process(m, METAL_BP_HZ, METAL_BP_Q, double_sr);
            metal += bp * 0.5;
        }
        let src = metal * METAL_GAIN + self.noise.sample() * NOISE_GAIN;

        let (_, _, bright) = self.hp.process(src, HP_HZ, HP_Q, sample_rate);
        let env = self.env.next();
        let attack_raw = (self.t / ATTACK_SECS).min(1.0);
        let attack = attack_raw * attack_raw * (3.0 - 2.0 * attack_raw);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        self.t += 1.0 / sample_rate;
        bright * env * self.gain * attack * fade * 0.85
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.gain = 0.0;
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }
}

/// Public instrument: monophonic voice with tail crossfade.
pub struct HiHat(Mono<HatVoice>);

impl HiHat {
    pub fn new(id: usize) -> Self {
        Self(Mono::new(id, InstrumentRole::Hat))
    }
}

impl Instrument for HiHat {
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
    fn closed_hat_is_audible() {
        // Regression "hi-hat inaudible in the mix": a closed tick at full
        // velocity must deliver substantial energy, not a whisper.
        let sr = 44_100.0;
        let mut hat = HiHat::new(5);
        hat.set_sample_rate(sr);
        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        let mut sum = 0.0;
        let mut n = 0;
        while hat.is_active() {
            let s = hat.next_sample();
            assert!(s.is_finite());
            sum += s.abs();
            n += 1;
        }
        assert!(n > 100, "tick too short: {n}");
        let mut peak: f32 = 0.0;
        // Second pass for the peak — retrigger the same tick.
        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        while hat.is_active() {
            peak = peak.max(hat.next_sample().abs());
        }
        assert!(
            sum / n as f32 > 0.005 && peak > 0.1,
            "hat too quiet: mean={} peak={peak}",
            sum / n as f32
        );
    }

    #[test]
    fn metal_lives_in_the_cymbal_band_not_at_nyquist() {
        // Classic 808 cymbal: energy at 7–10 kHz noticeably exceeds the low
        // band (100–1500 Hz). Previous squares at 13.6 kHz squealed at Nyquist
        // with an almost empty low end — now BP+HP hold the band.
        use super::super::band_ratio;
        let sr = 44_100.0;
        let mut hat = HiHat::new(5);
        hat.set_sample_rate(sr);
        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        let out: Vec<f32> = (0..(0.05 * sr) as usize)
            .map(|_| hat.next_sample())
            .collect();
        let high = band_ratio(&out, sr, 7000.0, 10_000.0);
        let low = band_ratio(&out, sr, 100.0, 1500.0);
        assert!(high > 0.05, "no cymbal band: {high:.4}");
        assert!(high > low * 2.0, "hat is a low thud: high={high} low={low}");
    }

    #[test]
    fn closed_and_open_differ_in_length() {
        let sr = 44_100.0;
        let mut hat = HiHat::new(5);
        hat.set_sample_rate(sr);

        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.05));
        let mut closed_n = 0;
        while hat.is_active() {
            assert!(hat.next_sample().is_finite());
            closed_n += 1;
        }

        hat.trigger(&NoteEvent::new(5, 42, 1.0, 0.3));
        let mut open_n = 0;
        while hat.is_active() {
            assert!(hat.next_sample().is_finite());
            open_n += 1;
        }

        assert!(open_n > closed_n * 3, "open {open_n} vs closed {closed_n}");
    }
}
