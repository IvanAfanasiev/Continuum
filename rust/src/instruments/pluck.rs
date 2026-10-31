use super::{
    dsp::{fast_tanh, note_seed, release_fade, saw_blep, square_blep, ExpDecay, Noise, Svf},
    sane_sample_rate, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Голос электро-плака: BLEP-пилы + BLEP суб-квадрат через резонансный
/// SVF с быстрым «щелчком» среза и мягким насыщением. Чистый современный
/// плак вместо резкого чиптюна. Короткий блип (хвост 0.35 с, как референс).
///
/// Тембр сознательно тёмный: тело плака живёт в 300–3000 Гц, резонанс
/// почти убран (Q 0.9), а после сатурации стоит ещё один мягкий срез.
/// Такой голос читается на телефоне, но не «звенит» и не дерет верх.
const SUB_GAIN: f32 = 0.22;
const DETUNE: f32 = 1.0025;
const FILTER_FLOOR_HZ: f32 = 360.0;
const FILTER_SNAP_HZ: f32 = 2400.0;
const FILTER_SNAP_RATE: f32 = 13.0;
const FILTER_Q: f32 = 0.9;
/// Мягкая полка после насыщения: гармоники сатурации не уходят в свист.
const TONE_LP_HZ: f32 = 6200.0;
const TONE_Q: f32 = 0.6;
const AMP_DECAY_RATE: f32 = 9.0;
const MAX_RING_SECS: f32 = 0.35;
const ATTACK_SECS: f32 = 0.003;
const FADE_SECS: f32 = 0.02;
const VOICE_GAIN: f32 = 0.78;
/// Щипок: скорость спада и своя рампа. Громкость — как медиатор:
/// слышно атаку, а не второй инструмент поверх струны.
/// Шум щипка не белый: полосовой «тик» 1.2–2.8 кГц. Широкополосный
/// всплеск и был главным источником дребезга (энергия до Найквиста).
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
    /// Мягкая полка после сатурации и полосовой фильтр щипка.
    tone: Svf,
    pick_filter: Svf,
    frequency: f32,
    gain: f32,
    t: f32,
    samples_left: usize,
    /// Рекуррентные огибающие вместо посэмпловых `exp`: один multiply
    /// на сэмпл, коэффициенты посчитаны в `trigger`.
    amp_env: ExpDecay,
    cutoff_env: ExpDecay,
    /// Щипок: короткий шумовой транзиент до мягкого тела.
    pick_env: ExpDecay,
    /// Шум щипка: пересеивается нотой (одинаковые щипки одинаковы).
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
        // Небольшое сид-зависимое смещение фаз: транзиенты не совпадают
        // бит-в-бит при повторах, но высота и огибающая сохраняются.
        let phase_seed = note_seed(note, 0x1234);
        self.phase_saw1 = (phase_seed as f32 / u32::MAX as f32) * 0.08;
        self.phase_saw2 = ((phase_seed >> 8) as f32 / u32::MAX as f32) * 0.08;
        self.phase_sub = ((phase_seed >> 16) as f32 / u32::MAX as f32) * 0.08;
        self.amp_env = ExpDecay::start(AMP_DECAY_RATE, self.sample_rate);
        self.cutoff_env = ExpDecay::start(FILTER_SNAP_RATE, self.sample_rate);
        // Щипок гаснет за ~7 мс: атака струны, а не второй инструмент.
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

        self.phase_saw1 = (self.phase_saw1 + self.frequency / sample_rate) % 1.0;
        self.phase_saw2 = (self.phase_saw2 + (self.frequency * DETUNE) / sample_rate) % 1.0;
        self.phase_sub = (self.phase_sub + (self.frequency * 0.5) / sample_rate) % 1.0;
        let saw = (saw_blep(self.phase_saw1, self.frequency, sample_rate)
            + saw_blep(self.phase_saw2, self.frequency * DETUNE, sample_rate))
            * 0.5;
        let sub = square_blep(self.phase_sub, self.frequency * 0.5, sample_rate) * SUB_GAIN;

        let cutoff = FILTER_FLOOR_HZ + FILTER_SNAP_HZ * self.cutoff_env.next();
        let (low, _, _) = self
            .filter
            .process(saw + sub, cutoff, FILTER_Q, sample_rate);
        // Сатурация до финальной полки: мягкое тело, но без гармоник,
        // которые царапают 5–10 кГц. Раньше та же цепь звучала стерильно
        // резко именно из-за неотфильтрованного щипка и открытого верха.
        let driven = fast_tanh(low * 1.05);
        let (voiced, _, _) = self.tone.process(driven, TONE_LP_HZ, TONE_Q, sample_rate);

        // Щипок идёт МИМО мягкой S-атаки тела (своя рампа): иначе
        // транзиент душится. Но это полосовой тик, а не белый клик:
        // узкая полоса не рассыпается в «дребезг» на телефоне.
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
        // Тембр плака: тело в 0.5–2 кГц, верх мягкий. Полоса 2–5 кГц —
        // «резкость/дребезг» на телефоне; после реворка ее доля мала.
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
        // Щипок идёт до тела: в первые 2 мс (пока S-атака почти закрыта)
        // уже есть широкополосный транзиент — медиатор слышно.
        // Детерминирован нотой: повтор — тот же щипок.
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
    fn pluck_is_a_short_blip() {
        // Референс — блип 0.1 с: нота 0.6 с обязана затихнуть к 0.45 с.
        let sr = 44_100.0;
        let mut pluck = VoicePool::plucks(6, 1);
        pluck.set_sample_rate(sr);
        pluck.trigger(&NoteEvent::new(6, 72, 1.0, 0.6));
        for _ in 0..(0.45 * sr) as usize {
            pluck.next_sample();
        }
        assert!(!pluck.is_active(), "pluck rings too long");
    }
}
