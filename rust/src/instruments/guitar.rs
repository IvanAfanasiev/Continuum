use super::{
    dsp::{release_fade, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Чистая электрогитара: струна Карплуса-Стронга с мягким щипком,
/// расгеадо веером и глушением ghost-нот. Синусы не стали струной —
/// только физическая модель дает струнный спад (верх умирает первым
/// сам по себе) и живой раздрай атаки. Для лоуфай-боя поверх Rhodes
/// и рок-риффов. Полифония 6.
/// Длительность сустейна струны (открытые струны звенят, глушеные — нет).
const RING_T60_SECS: f32 = 1.1;
const RING_T60_MUTED_SECS: f32 = 0.12;
const MAX_KS_LEN: usize = 2048;
const LP_HZ: f32 = 3400.0;
const LP_MUTED_HZ: f32 = 1800.0;
const LP_Q: f32 = 0.7;
const ATTACK_SECS: f32 = 0.006;
/// Тише этого — ghost-нота с приглушенными струнами.
const MUTE_VELOCITY: f32 = 0.6;
/// Расгеадо: сдвиги ударов и их акценты (сумма нормируется).
/// 16 мс между струнами — штрихи слышны раздельно, как бой,
/// а не один удар с тремоло.
const STRUM_GAP_SECS: f32 = 0.016;
const STRUM_GAP_MUTED_SECS: f32 = 0.008;
const STRUM_ACCENTS: [f32; 3] = [1.0, 0.7, 0.5];
const STRUM_NORM: f32 = 1.0 / (1.0 + 0.7 + 0.5);
const MAX_RING_SECS: f32 = 1.5;
const FADE_SECS: f32 = 0.05;
/// Выход norмирован под микс: струна тише синта, но слышна (тест-пин).
const VOICE_GAIN: f32 = 0.9;

#[derive(Clone, Default)]
pub struct GuitarVoice {
    sample_rate: f32,
    ks_buf: Vec<f32>,
    ks_pos: usize,
    ks_len: usize,
    damp: f32,
    filter: Svf,
    noise: Noise,
    frequency: f32,
    gain: f32,
    muted: bool,
    t: f32,
    samples_left: usize,
}

impl Voice for GuitarVoice {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sane_sample_rate(sample_rate);
    }

    fn trigger(&mut self, note: &NoteEvent) {
        let sample_rate = self.sample_rate;
        self.frequency = midi_to_freq(note.midi_pitch);
        let ring = note.duration_secs.clamp(0.05, MAX_RING_SECS);
        self.samples_left = (ring * sample_rate) as usize;
        self.gain = note.velocity;
        // Тихие ghost-ноты играются с глушением струн.
        self.muted = note.velocity < MUTE_VELOCITY;
        self.t = 0.0;
        // Струна: линия задержки на период тона + затухание под T60.
        // Глушеные звенят 0.12 с (ладонь), открытые — 1.1 с.
        self.ks_len = (sample_rate / self.frequency.max(20.0)).round() as usize;
        self.ks_len = self.ks_len.clamp(8, MAX_KS_LEN);
        let t60 = if self.muted {
            RING_T60_MUTED_SECS
        } else {
            RING_T60_SECS
        };
        self.damp = (-6.908 * self.ks_len as f32 / (t60 * sample_rate)).exp();
        // Мягкий щипок: шум через однополюсный lowpass — округлый фронт
        // без щелчка медиатора. Нормируем к полному размаху: мягкость
        // дает фильтр, а не тишина (иначе струны не слышно в миксе).
        self.noise = Noise::new(super::dsp::note_seed(note, 0x6174));
        self.ks_buf.resize(self.ks_len, 0.0);
        let mut lp = 0.0f32;
        let mut peak = 0.0f32;
        for b in self.ks_buf.iter_mut() {
            let n = self.noise.sample();
            lp += 0.5 * (n - lp);
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
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 || self.ks_len == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let a = (self.t / ATTACK_SECS).min(1.0);
        let attack = a * a * (3.0 - 2.0 * a);
        let fade = release_fade(self.samples_left, (FADE_SECS * sample_rate) as usize);

        // Расгеадо: веер из трех ударов со спадающими акцентами.
        let gap = if self.muted {
            STRUM_GAP_MUTED_SECS
        } else {
            STRUM_GAP_SECS
        };
        let mut strum = 0.0;
        for (k, accent) in STRUM_ACCENTS.iter().enumerate() {
            let tau = self.t - k as f32 * gap;
            if tau > 0.0 {
                let u = (tau / ATTACK_SECS).min(1.0);
                strum += accent * u * u * (3.0 - 2.0 * u);
            }
        }
        strum *= STRUM_NORM;

        // Карплус-Стронг: усреднение соседей гасит верх само собой —
        // струна темнеет по мере звона, как живая.
        let cur = self.ks_buf[self.ks_pos];
        let nxt = self.ks_buf[(self.ks_pos + 1) % self.ks_len];
        self.ks_buf[self.ks_pos] = self.damp * 0.5 * (cur + nxt);
        self.ks_pos = (self.ks_pos + 1) % self.ks_len;

        let cutoff = if self.muted { LP_MUTED_HZ } else { LP_HZ };
        let (warm, _, _) = self.filter.process(cur, cutoff, LP_Q, sample_rate);

        self.t += 1.0 / sample_rate;
        warm * self.gain * attack * fade * strum * VOICE_GAIN
    }

    fn is_active(&self) -> bool {
        self.samples_left > 0
    }

    fn reset(&mut self) {
        self.samples_left = 0;
        self.t = 0.0;
        self.gain = 0.0;
        self.filter.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Instrument, VoicePool};
    use super::*;

    fn guitars() -> VoicePool {
        VoicePool::guitars(16, 6)
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
    fn muted_ghost_dies_fast() {
        // Тихая нота (ghost) глушится: во второй половине почти тишина,
        // тогда как открытая в том же окне еще звучит.
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
