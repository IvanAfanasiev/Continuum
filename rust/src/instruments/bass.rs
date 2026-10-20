use super::{
    dsp::{fast_tanh, release_fade, sin01, ExpDecay, Noise, Svf},
    sane_sample_rate, Instrument, InstrumentRole, Mono, NoteEvent, Voice,
};
use crate::core::midi_to_freq;

/// Глубокий саб-бас: один синусный осциллятор, два тракта.
/// Чистый саб дает глубину (синус не умеет дребезжать и алиасить),
/// мягкое насыщение той же волны — теплое присутствие на мелких
/// динамиках (мозг восстанавливает фундамент по гармоникам).
/// Четная октава (+12 полутонов, ~0.3 от фундамента) — ламповая
/// теплота как в референсах: нечетный tanh в одиночку дает
/// только «пустой» квадратный тембр.
/// Никаких расстроенных пил и резонанса: именно они давали
/// «клавишный» дребезг прошлой версии.
const SUB_GAIN: f32 = 0.9;
const OCTAVE_GAIN: f32 = 0.5;
const PRESENCE_GAIN: f32 = 0.45;
/// Драйв волны. Было 1.5 + 2.5·velocity (до 3.75): такая перегрузка
/// давала почти квадратный тембр — грубый «хрип» на самом низу.
/// Теперь мягкое насыщение: тепло есть, скрежета нет.
const DRIVE_BASE: f32 = 1.2;
const DRIVE_VEL: f32 = 1.3;
/// Нормировка выхода. Сумма слоев баса (саб + октава + презенс + щипок)
/// доходила до 1.7, и бас сам клиппил в движке: пик 1.35 при 2675
/// срезанных сэмплах — это и есть хрип на низу. Нормируем так, чтобы
/// одна нота и перекрытие с хвостом оставались в единице.
const OUT_GAIN: f32 = 0.47;
const PRESENCE_Q: f32 = 0.7;
/// Щипок медиатора: короткий шумовой транзиент. Его слышно
/// в picked-басу референса; без него атака безликая.
const PICK_DECAY: f32 = 120.0;
const PICK_GAIN: f32 = 0.12;
const PITCH_DROP: f32 = 0.025;
const PITCH_DROP_RATE: f32 = 80.0;
/// Атака с фейдом вместо щелчка: 10 мс + мягкость сида сверху.
/// Релиз длинный (35 мс): бас допевает, а не чмокает.
const ATTACK_SECS: f32 = 0.010;
const AMP_DECAY_RATE: f32 = 8.0;
const SUSTAIN_LEVEL: f32 = 0.85;
const FADE_SECS: f32 = 0.035;

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
    /// Живая высота и громкость: легато скользит к цели (~30/5 мс),
    /// а не прыгает — шагающая бас-линия течет, а не рубится.
    freq_now: f32,
    gain_target: f32,
    /// Рекуррентные огибающие и коэффициенты скольжения: посчитаны
    /// в `trigger`/`set_sample_rate`, дальше без единого `exp`/`sin`.
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
        self.phase = 0.0;
        self.elapsed = 0;
        self.attack_mul = note.attack_mul;
        self.release_mul = note.release_mul;
        self.sub_bias = note.sub_bias.clamp(0.0, 1.0);
        self.noise = Noise::new(super::dsp::note_seed(note, 0x8A55));
        self.amp_env = ExpDecay::start(AMP_DECAY_RATE, self.sample_rate);
        self.drop_env = ExpDecay::start(PITCH_DROP_RATE, self.sample_rate);
        self.cutoff_env = ExpDecay::start(1.0 / 0.3, self.sample_rate);
        self.pick_env = ExpDecay::start(PICK_DECAY, self.sample_rate);
        self.update_rate_ks();
        self.presence_lp.reset();
        self.presence_lp2.reset();
    }

    fn has_legato(&self) -> bool {
        true
    }

    fn legato(&mut self, note: &NoteEvent) {
        // Новая высота без перезапуска: фаза и огибающая текут,
        // щипок не пересеивается (уже отзвучал), фильтры не рвутся.
        // Только цель скольжения, продление и свежая артикуляция сида.
        self.frequency = midi_to_freq(note.midi_pitch);
        let want = (note.duration_secs * self.sample_rate).max(1.0) as usize;
        self.samples_left = self.samples_left.max(want);
        self.gain_target = note.velocity;
        self.velocity = note.velocity;
        self.attack_mul = note.attack_mul;
        self.release_mul = note.release_mul;
        self.sub_bias = note.sub_bias.clamp(0.0, 1.0);
    }

    fn next_sample(&mut self) -> f32 {
        if self.samples_left == 0 {
            return 0.0;
        }
        self.samples_left -= 1;

        let sample_rate = self.sample_rate;
        let t = self.elapsed as f32 / sample_rate;
        self.elapsed += 1;

        // Легато-скольжение: высота едет к цели ~30 мс, громкость ~5 мс.
        // Фаза не рвется (интегратор тот же), атака не перезапускается.
        // Коэффициенты посчитаны заранее (см. trigger).
        self.freq_now += (self.frequency - self.freq_now) * self.glide_k;
        self.gain += (self.gain_target - self.gain) * self.gain_k;

        // Мягкая S-атака вместо линейной: ни щелчка, ни ватности.
        // Длина атаки — от сида: одни сиды punchier, другие ватнее.
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
        // Четная октава строго в фазе с фундаментом (та же фаза ×2):
        // тепло без биений — гармоники совпадают, а не спорят.
        let octave = sin01(self.phase * 2.0) * OCTAVE_GAIN;

        let sub = wave * SUB_GAIN;
        // Та же волна через драйв: гармоники идеально когерентны
        // фундаменту — тепло без биений и хоруса.
        // Саб-перекос стиля гасит презенс-цепь (край «струны») в пользу
        // чистого синуса: club глубокий и ровный, рок струнный (0.0).
        // Конверт среза (deep house): tanh-гармоники ныряют 680→300 Гц
        // за ~300 мс (щипок в атаке), а четная октава идет МИМО фильтра —
        // ламповое тепло стабильно весь сустейн, а не гаснет с нырком.
        let drive = DRIVE_BASE + DRIVE_VEL * self.velocity;
        let cutoff = 300.0 + 380.0 * self.cutoff_env.next();
        let (presence, _, _) =
            self.presence_lp
                .process(fast_tanh(wave * drive), cutoff, PRESENCE_Q, sample_rate);
        // Второй каскад (24 дБ/окт суммарно): одиночный пропускал
        // tanh-гармоники далеко вверх — нырка не было слышно.
        let (presence, _, _) = self
            .presence_lp2
            .process(presence, cutoff, PRESENCE_Q, sample_rate);

        let keep = 1.0 - 0.85 * self.sub_bias;
        // Щипок: широкополосный транзиент только в атаке (8 мс).
        // С перекосом тише, но не исчезает: picked-суб тоже щиплет.
        // Своя быстрая рампа (2 мс): щелчок идет раньше мягкого тела,
        // иначе 10 мс атака его душит.
        let pa = (t / 0.002).min(1.0);
        let pick = self.noise.sample()
            * self.pick_env.next()
            * PICK_GAIN
            * (1.0 - 0.5 * self.sub_bias)
            * pa;
        (sub + (presence * PRESENCE_GAIN + octave * OCTAVE_GAIN) * keep + pick)
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

/// Публичный инструмент: моно-голос с кроссфейдом хвоста.
/// Ретриггер на 16-х дропа не рубит сустейн ступенькой.
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
        // Конверт среза: первые 100 мс ярче (щипок), 400–500 мс темнее.
        // Меряем энергию первой разности (ВЧ) во временной области —
        // без утечек прямоугольного окна гёртцеля.
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
        // Регрессия «в басу нет басов»: корень C2 (65 Гц) обязан давать
        // весомый сустейн, а не шепот.
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
        // A2 = ровно 110 Гц (целая корзина гёртцеля 1 Гц): энергия
        // фундамента обязана кратно превышать верхнюю середину.
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
        // Перекос 1.0 давит презенс-цепь: 2-я гармоника падает ниже
        // музыкального порога, фундамент цел, все конечно.
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
            second1 < fund1 * 0.12,
            "sub not pure: 2nd={second1} fund={fund1}"
        );
        assert!(
            fund1 > fund0 * 0.5,
            "fundamental collapsed: {fund1} vs {fund0}"
        );
    }

    #[test]
    fn even_harmonic_adds_tube_warmth() {
        // Референс (Bad Angel, C2 65 Гц): 2-я гармоника ~0.3 от фундамента.
        // Чистый синус + tanh дают только нечетные — отсюда «плоскость».
        // Меряем на A2 (110 Гц, целая корзина): 2-я = ровно 220 Гц.
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
        // A2 = ровно 110 Гц: третья гармоника (330 Гц, тоже целый бин)
        // обязана быть умеренной. У пилы она 0.33 от фундамента —
        // отсюда и дребезг; у мягкого насыщения кратно меньше.
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
        // Мягкий сид (длинная атака) дает меньше энергии в первые 20 мс,
        // чем резкий — артикуляция реально работает, а не игнорируется.
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
        // Длинная линия не перебивает сама себя: вторая высота берется
        // без перезапуска атаки — энергия не падает в ноль, высота
        // доезжает до новой ноты.
        let sr = 44_100.0;
        let rms30 =
            |buf: &[f32]| (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        // Свежая атака: первые 30 мс тише сустейна (рампа с нуля).
        let mut fresh = BassGuitar::new(1);
        fresh.set_sample_rate(sr);
        fresh.trigger(&NoteEvent::new(1, 33, 0.8, 1.0));
        let mut fresh_buf = Vec::new();
        for _ in 0..(0.03 * sr) as usize {
            fresh_buf.push(fresh.next_sample());
        }
        // Сустейн перед переходом.
        let mut sus_buf = Vec::new();
        for _ in 0..(0.17 * sr) as usize {
            sus_buf.push(fresh.next_sample());
        }
        let sus = rms30(&sus_buf);
        let fresh_start = rms30(&fresh_buf);
        assert!(fresh_start < sus * 0.9, "attack does not ramp?");
        // Легато на другую высоту: провала нет, ступеньки нет.
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
        // Высота доехала: late — уже новая нота (C2 ~65 Гц ± допуск).
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
        // Ретриггер в громкий сустейн (16-е дропа) не должен давать
        // ступеньку: хвост гаснет кроссфейдом поверх новой атаки.
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
