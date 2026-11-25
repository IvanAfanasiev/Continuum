/// Finite-state machine of the composition structure.
//
// Active styles run an endless loop without a finale or a build-up
// (Outro contradicts "endless" generation, Intro contradicts the requirement
// to play at full strength right away): MainMotif -> BuildUp -> Drop -> PostChorus
// -> around again. The first bar is already the theme at full strength.
// Calm styles breathe with the theme: MainMotif almost always,
// rare BuildUp/PostChorus for variety.
//
// The seed's motif is fixed for the whole loop: the same theme
// develops through the form rather than being composed anew. Loops differ in
// section lengths and energy breathing, the melodic DNA is stable.
use crate::instruments::InstrumentRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SongSection {
    MainMotif,
    BuildUp,
    Drop,
    PostChorus,
}

/// Balance of parts by section: how loudly a role sounds.
/// Textures run as an even layer always; the rest breathes with the form.
/// Column order = the declaration order of [`InstrumentRole`] discriminants.
const STAGE_GAINS: [[f32; 8]; 4] = [
    // MainMotif
    [0.85, 0.9, 0.8, 0.85, 0.8, 0.75, 0.65, 1.0],
    // BuildUp
    [0.7, 0.9, 0.9, 0.9, 0.7, 0.8, 0.7, 1.0],
    // Drop
    [1.0, 1.0, 1.0, 1.0, 0.8, 1.0, 0.8, 1.0],
    // PostChorus: the groove rolls on (no restart!), only the top rests.
    [0.8, 0.9, 0.7, 0.85, 0.75, 0.55, 0.5, 1.0],
];

/// Score: an endless cycle of sections without build-up or finale.
#[derive(Debug, Clone, Copy)]
pub struct Score {
    pub cycle: &'static [(SongSection, usize)],
}

/// Full form of active styles: theme → build-up → hit → breakdown.
pub const ENERGETIC_SCORE: Score = Score {
    cycle: &[
        (SongSection::MainMotif, 8),
        (SongSection::BuildUp, 4),
        (SongSection::Drop, 8),
        (SongSection::PostChorus, 4),
    ],
};

/// Calm form: the theme breathes, but the chunk still reaches the hit
/// (in lofi that's a timbre change, not an explosion) and returns to the theme.
pub const CALM_SCORE: Score = Score {
    cycle: &[
        (SongSection::MainMotif, 8),
        (SongSection::BuildUp, 4),
        (SongSection::Drop, 8),
        (SongSection::PostChorus, 4),
        (SongSection::MainMotif, 6),
    ],
};

/// Deep house form: the grid never stops, the hit is long, the breakdown
/// is short. Even house-club: the theme holds the beat, the build-up is short, the drop
/// is stretched — in club the drop isn't an explosion but eight bars of one grid.
pub const HOUSE_SCORE: Score = Score {
    cycle: &[
        (SongSection::MainMotif, 8),
        (SongSection::BuildUp, 4),
        (SongSection::Drop, 12),
        (SongSection::PostChorus, 4),
    ],
};

/// Rock form: the riff arrives quickly, the build-up is short, the hit is long.
/// Rock is held by the riff, not the grid: the theme is short so the riff
/// starts right away, and the drop is long — that's where the solo and the second voice are.
pub const ROCK_SCORE: Score = Score {
    cycle: &[
        (SongSection::MainMotif, 6),
        (SongSection::BuildUp, 2),
        (SongSection::Drop, 10),
        (SongSection::PostChorus, 4),
    ],
};

/// Funk form: drift (the theme is long and hypnotic), a short build-up,
/// a medium hit, a long breakdown — half-time and emptiness.
pub const DRIFT_SCORE: Score = Score {
    cycle: &[
        (SongSection::MainMotif, 12),
        (SongSection::BuildUp, 4),
        (SongSection::Drop, 8),
        (SongSection::PostChorus, 8),
    ],
};

/// Proto form: a bare grid with no concessions. The theme is long and hypnotic,
/// the build-up is short, the drop long, the breakdown short — in proto a break
/// is a change of rhythm, not a rest.
pub const PROTO_SCORE: Score = Score {
    cycle: &[
        (SongSection::MainMotif, 10),
        (SongSection::BuildUp, 4),
        (SongSection::Drop, 10),
        (SongSection::PostChorus, 2),
    ],
};

/// Choose a style's form by seed: deterministic, stateless.
pub fn pick_score(seed: u64, salt: u64, variants: &'static [Score]) -> Score {
    if variants.is_empty() {
        return ENERGETIC_SCORE;
    }
    let idx = (splitmix64(seed ^ salt) % variants.len() as u64) as usize;
    variants[idx]
}

/// The musical phrase changes every N bars.
const PHRASE_LENGTH_MEASURES: usize = 2;
/// Phrase shift between loops: 3 is coprime with both 7 and 5 —
/// the melody doesn't repeat in 7-degree scales nor in pentatonic.
const CYCLE_PHRASE_JUMP: usize = 3;

#[derive(Clone, Debug)]
pub struct SongContext {
    pub section: SongSection,
    /// Absolute bar number from the session start (1-based). It doesn't reset
    /// at section boundaries: the progression, the second half of the
    /// motif and the phase of the multi-bar riff cell live by it (see `styles::play_guide`).
    pub measure_index: usize,
    /// Monotonic musical phrase number (2 bars). Phrases are aligned
    /// to odd bars: `total=1,2 -> 0; 3,4 -> 1; ...`. Within a phrase
    /// the bar phase = `(measure_index - 1) % 2`.
    pub phrase_index: usize,
    pub energy: f32,
    pub progress: f32,
    pub is_last_measure: bool,
    /// Bar number within the current section (1-based). Transitions and
    /// the poles crossfade know that the section has just begun, not merely
    /// that the absolute bar changed.
    pub section_measure_index: usize,
    /// Previous form section. On the first section and while holding
    /// the last section it equals the current one.
    pub previous_section: SongSection,
    /// Loop degree 0..8 (the theme occurs at 1 and 5).
    pub stage: usize,
    /// Form loop number (0 = first pass).
    pub cycle: u64,
    /// Role volumes at this degree, including edge fades.
    pub stage_gains: [f32; 8],
    /// Who plays in this section (group mask): the arrangement is written in the chunk
    /// plan, not tossed on every step.
    pub layers: crate::piece::LayerMask,
    /// Seed hash: scale rotation and modal variation in generation.
    /// Stable within a seed — the key doesn't jump mid-track.
    pub seed_hash: u64,
    /// Generation density 0.7..=1.3 from the seed: sparse and dense seeds.
    pub density: f32,
    /// Seed motif: 16 scale degrees 0..4 — the track's melodic DNA.
    /// The long theme two bars later (phrase window) as a narrow chant:
    /// seeds differ by theme, not by random jumps.
    /// Wide leaps gave "their own composition" to every layer —
    /// an inaudible mush instead of heterophony.
    /// Created in the intro and NOT changed for the whole loop: one seed = one
    /// recognizable theme developed by the form, not recomposed.
    pub motif: [u8; 16],
    /// The seed's second theme: B is a variation of A (3 mutations), the material of climaxes.
    /// Drop and PostChorus sing B, the rest A: verse/chorus differ
    /// by theme, not only by volume. Boundaries are section boundaries (their lengths
    /// come from the seed), there's no separate block logic.
    pub motif_b: [u8; 16],
    /// B sounds at the climaxes (Drop/PostChorus), A everywhere else.
    pub use_alt_motif: bool,
    /// Harmonic progression of the seed: degree shifts 0..4 for accompaniment,
    /// changing every 2 bars. The lead holds the motif, the harmony moves —
    /// seeds differ both in theme and in harmonic motion.
    pub prog: [u8; 4],
    /// Raw tonic wandering (prefix sum of −1/0/+1 steps for pairs of
    /// visits `0..stage/2`). The consumer applies `% scale_len` itself.
    /// Incrementally: generation is endless, recalculating from scratch every step is
    /// not an option (see `Arranger::extend_tonic`).
    pub tonic_walk: i64,
}

impl SongContext {
    /// A role's volume at the current degree (1.0 = as written in the note).
    pub fn stage_gain(&self, role: InstrumentRole) -> f32 {
        self.stage_gains[role.index()]
    }
}

#[derive(Clone)]
pub struct Arranger {
    /// Chunk plan: a finite form (30-60 s), not an endless loop.
    /// Sections follow the plan; the seam at the end is a written finale.
    plan: crate::piece::PiecePlan,
    /// Position within the plan: the body's section index.
    plan_idx: usize,
    /// Plan exhausted: body and seam have played, waiting for the next chunk.
    plan_done: bool,
    /// Current section of the plan (from the plan, not the skeleton loop).
    section: SongSection,
    /// The section that played before the current one: needed for transitions (the
    /// tail of a gesture after BuildUp) and the poles crossfade on the first bar.
    previous_section: SongSection,
    /// Written energy of the section (`None` — from the general form formula).
    section_energy: Option<f32>,
    /// Who plays in the section (group mask) — from the chunk plan.
    section_layers: crate::piece::LayerMask,
    /// How the style glues chunks (smooth overlap or sharp join).
    transition: crate::piece::Transition,
    /// Style arrangement: who plays in each section. Needed so that the next
    /// chunk is planned with the same arrangement.
    layers: crate::piece::Layers,
    /// Chunk number in the stream: chunks differ from one another.
    piece: u64,
    /// Absolute bar on which the current chunk started: from it we compute
    /// how many bars of the chunk have already played.
    piece_start_measure: usize,
    /// Chunk seed (raw): the planner salts the form with it.
    seed: u64,
    /// Style tempo: the chunk's length in bars depends on it.
    bpm: f32,
    /// Style skeleton: the following chunks are planned from it.
    score: Score,
    /// Plan of the next chunk: written when the current chunk's seam starts.
    next_piece: Option<crate::piece::PiecePlan>,
    section_measure_count: usize,
    section_len: usize,
    total_measures: usize,
    stages_done: u64,
    seed_hash: u64,
    cycle: u64,
    phrase_offset: usize,
    density: f32,
    motif: [u8; 16],
    motif_b: [u8; 16],
    prog: [u8; 4],
    /// Smoothed role volumes: they travel halfway to the table over a bar,
    /// rather than jumping at joins (instruments don't fade out or burst in).
    prev_gains: [f32; 8],
    /// Incremental tonic wandering: prefix sum of steps for pairs
    /// of visits `0..tonic_done`. Stages are monotonic, so the walk only
    /// extends — every generation step costs O(1), not O(stage).
    /// The raw sum (not modulo): the consumer applies the scale modulus,
    /// scales may differ between styles.
    tonic_pos: i64,
    tonic_done: usize,
}

/// Deterministic 64-bit hash (splitmix64).
pub(crate) fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Hash of three numbers into [0, 1).
fn hash01(a: u64, b: u64, c: u64) -> f32 {
    let h = splitmix64(a ^ b.wrapping_mul(0x9E3779B9) ^ c.wrapping_mul(0x85EBCA6B));
    (h >> 32) as f32 / (u32::MAX as f32)
}

/// Rebuild of theme B and the progression for every chunk of the loop. The first loop
/// (cycle 0) keeps the foundation as-is; further on the seed + loop number
/// give up to two point mutations in the middle of the theme (the hook 0..3 and the cadence
/// 12..15 are untouched) and a cyclic shift of the progression — the same set
/// of chords in a different order. Chunks differ not only in length, but
/// the seed's DNA and key are preserved.
pub(crate) fn remix_for_cycle(
    motif_b: [u8; 16],
    prog: [u8; 4],
    seed_hash: u64,
    cycle: u64,
) -> ([u8; 16], [u8; 4]) {
    if cycle == 0 {
        return (motif_b, prog);
    }
    let salt = splitmix64(seed_hash ^ cycle.wrapping_mul(0x9E37_79B9));
    let mut b = motif_b;
    let mut h = salt;
    for _ in 0..2 {
        h = splitmix64(h);
        let pos = 4 + (h % 8) as usize;
        h = splitmix64(h);
        let d = if h.is_multiple_of(2) { 1i8 } else { -1 };
        let v = (b[pos] as i8 + d).clamp(0, 4);
        if (v - b[pos - 1] as i8).abs() <= 1 && (v - b[pos + 1] as i8).abs() <= 1 {
            b[pos] = v as u8;
        }
    }
    let rot = (splitmix64(salt ^ 0x000A_11CE) % 4) as usize;
    let mut p = prog;
    p.rotate_left(rot);
    (b, p)
}

/// One step of the deterministic tonic walk (−1/0/+1 from the bits
/// of the hash of a pair of visits). A step is at most one degree per pair (musically
/// coherent), the path is non-periodic (the stream's period is 2^64 — the skeleton
/// never closes). Same seed — same path.
/// Atom of generation: the full walk is accumulated incrementally
/// in [`Arranger::extend_tonic`], tests accumulate it manually.
pub(crate) fn drift_step(seed_hash: u64, pair: usize) -> i64 {
    let h = splitmix64(seed_hash ^ (pair as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD21F7);
    match h % 3 {
        0 => 0,
        1 => 1,
        _ => -1,
    }
}

/// Harmonic progressions (scale degrees, all diatonic — any
/// combination stays in the scale). The choice is by seed: accompaniment
/// of different seeds moves differently with the same lead motif.
/// The pool is wide (8): seeds clearly differ in harmonic motion.
/// Public within the crate: a style may provide its own pool (see `Style::progressions`).
pub(crate) const PROGS: [[u8; 4]; 8] = [
    [0, 2, 3, 1],
    [0, 3, 2, 3],
    [2, 3, 0, 1],
    [3, 1, 0, 2],
    [1, 3, 0, 2],
    [2, 0, 1, 3],
    [0, 1, 2, 3],
    [3, 0, 2, 1],
];

/// Visit length in this loop: base or base + 2 bars.
/// Drops aren't varied — they're the form's anchors. `salt` is unique per visit.
fn varied_len(section: SongSection, base: usize, seed_hash: u64, salt: u64) -> usize {
    if section == SongSection::Drop {
        return base;
    }
    let extra = (splitmix64(seed_hash ^ salt) % 2) as usize * 2;
    base + extra
}

/// Section volumes from the table. Joins glide in advance_measure,
// nobody fades out or bursts in suddenly.
fn stage_gains_for(section: SongSection) -> [f32; 8] {
    STAGE_GAINS[section as usize]
}

impl Arranger {
    pub fn new(seed: u64) -> Self {
        Self::with_score(seed, ENERGETIC_SCORE)
    }

    pub fn with_score(seed: u64, score: Score) -> Self {
        Self::with_score_and_layers(seed, score, crate::piece::default_layers())
    }

    /// Form with the style's arrangement: who plays in each section.
    pub fn with_score_and_layers(seed: u64, score: Score, layers: crate::piece::Layers) -> Self {
        // Default form: a chunk from the style's skeleton. Default tempo —
        // the style knows its own and substitutes its plan (see `with_plan`).
        let plan = crate::piece::plan_piece(
            seed,
            126.0,
            score.cycle,
            0,
            0,
            crate::piece::Transition::Rolling { bars: 2 },
            layers,
        );
        Self::with_plan(
            seed,
            126.0,
            score,
            plan,
            crate::piece::Transition::Rolling { bars: 2 },
            layers,
        )
    }

    /// Arranger with a ready chunk plan: the form is finite (30-60 s), the seam at
    /// the end is a written finale. The arranger plans the following chunks itself
    /// when a seam starts: the join between chunks is written in advance.
    pub fn with_plan(
        seed: u64,
        bpm: f32,
        score: Score,
        plan: crate::piece::PiecePlan,
        transition: crate::piece::Transition,
        layers: crate::piece::Layers,
    ) -> Self {
        let mut a = Self::with_plan_inner(seed, bpm, score, plan, transition, layers);
        a.enter_section();
        a
    }

    fn with_plan_inner(
        seed: u64,
        bpm: f32,
        score: Score,
        plan: crate::piece::PiecePlan,
        transition: crate::piece::Transition,
        layers: crate::piece::Layers,
    ) -> Self {
        let seed_hash = splitmix64(seed ^ 0x51ED);
        // The theme's range is the seed's handwriting: half the seeds sing narrowly
        // (stepwise, as before), half widely (leaps up to
        // a third). One chant for all gave "identical generation";
        // now the theme's character (chant vs leaps) distinguishes seeds.
        let wide = splitmix64(seed_hash ^ 0x1DE5).is_multiple_of(2);
        // The seed's motif: a chant-like line 0..4 (a narrow chant),
        // rather than 16 independent rolls. A random walk with step ≤ 1
        // (wide seeds — up to 2) with an arch (rise toward the middle, fall
        // toward the cadence): the melody is coherent within a seed, developed by the
        // phrase window and the tonic, not re-rolled each step. Generation is
        // backwards from the tonic: the last step is always 0 (cadence),
        // the penultimate is a neighbor (a proper approach, not a leap).
        // It lives unchanged for the whole loop.
        let mut motif = [0u8; 16];
        let mut m = splitmix64(seed_hash ^ 0x9071F);
        motif[15] = 0;
        let mut pos: i8 = 0;
        let mut dir: i8 = 1;
        for i in (0..15).rev() {
            m = splitmix64(m);
            let r = m % 100;
            if r >= 20 {
                // Direction changes are rare (a chant, not a sawtooth).
                if r >= 94 {
                    dir = -dir;
                }
                // Arch: toward the middle we pull up, toward the beginning — toward the middle
                // of the range (the peak is somewhere in the first half of the way back).
                if (6..11).contains(&i) && pos < 3 {
                    dir = 1;
                }
                if i < 4 && pos > 2 {
                    dir = -1;
                }
                if i < 4 && pos < 2 {
                    dir = 1;
                }
                // Wide seeds sometimes step over a degree (a third).
                // If it doesn't fit — fall back to stepwise (always valid).
                let mut next = pos + dir;
                if wide && (m >> 32).is_multiple_of(4) {
                    let ext = pos + dir * 2;
                    if (0..=4).contains(&ext) {
                        next = ext;
                    }
                }
                if !(0..=4).contains(&next) {
                    dir = -dir;
                    next = pos + dir;
                }
                pos = next.clamp(0, 4);
            }
            motif[i] = pos as u8;
        }
        // Theme B: a close variation of A — up to 3 mutations (±a degree, for wide
        // seeds up to a third). Not "a second composition", but development of the same one:
        // a chorus to the verse, not another track. Positions come from the seed's hash,
        // the step is always neighboring (clamp instead of modulo: modulo gave
        // a 4→0 jump).
        // The mutation is the middle of the theme (cells 1–2): the hook at the start and the cadence
        // at the end of B hold, while the middle answers. So the Drop differs
        // by development, not by a foreign melody.
        let mut motif_b = motif;
        let mut g = splitmix64(seed_hash ^ 0xB17B);
        // Line tolerance: a wide seed tolerates leaps up to 2, a narrow one — up to 1.
        let line_lim: i8 = if wide { 2 } else { 1 };
        for _ in 0..3 {
            g = splitmix64(g);
            // The middle 4..11: the hook (0..3) and the cadence (12..15) are untouched.
            let pos = 4 + (g % 8) as usize;
            g = splitmix64(g);
            let cur = motif_b[pos] as i8;
            let left = motif_b[pos - 1] as i8;
            let right = motif_b[pos + 1] as i8;
            let mut done = false;
            // Wide ones mutate by thirds (±2 as the first candidate),
            // narrow ones stepwise: B within A's range.
            let mag: i8 = if wide { 2 } else { 1 };
            for d in [
                if g.is_multiple_of(2) { mag } else { -mag },
                if g.is_multiple_of(2) { -1 } else { 1 },
            ] {
                let v = (cur + d).clamp(0, 4);
                if (v - left).abs() <= line_lim && (v - right).abs() <= line_lim {
                    motif_b[pos] = v as u8;
                    done = true;
                    break;
                }
            }
            let _ = done;
        }
        if motif_b == motif {
            // Guard: we look for a middle where the step fits the line. The hook and
            // the cadence are left alone even here.
            'guard: for pos in 4..12 {
                let left = motif_b[pos - 1] as i8;
                let right = motif_b[pos + 1] as i8;
                for d in [1i8, -1] {
                    let v = (motif_b[pos] as i8 + d).clamp(0, 4);
                    if (v - left).abs() <= line_lim && (v - right).abs() <= line_lim {
                        motif_b[pos] = v as u8;
                        break 'guard;
                    }
                }
            }
        }
        let first_section = plan
            .body
            .first()
            .map(|s| s.section)
            .unwrap_or(SongSection::MainMotif);
        Self {
            plan,
            plan_idx: 0,
            plan_done: false,
            section: first_section,
            previous_section: first_section,
            section_energy: None,
            section_layers: layers[crate::piece::layer_index(first_section)],
            transition,
            layers,
            piece: 0,
            piece_start_measure: 0,
            seed,
            bpm,
            score,
            next_piece: None,
            section_measure_count: 0,
            section_len: 0,
            total_measures: 0,
            stages_done: 0,
            seed_hash,
            cycle: 0,
            // The phrase's starting shift + density — from the seed, stable for the whole track.
            phrase_offset: (seed_hash % 8) as usize,
            density: 0.7 + 0.6 * hash01(seed_hash, 0xD517, 0),
            motif,
            motif_b,
            prog: PROGS[(seed_hash % PROGS.len() as u64) as usize],
            // Start at full strength: the first bar is already the theme, no build-up.
            prev_gains: stage_gains_for(first_section),
            tonic_pos: 0,
            tonic_done: 0,
        }
    }

    /// Plan section by index: the body, then the seam (chunk finale).
    fn plan_section(&self, idx: usize) -> Option<&crate::piece::SectionPlan> {
        if idx < self.plan.body.len() {
            self.plan.body.get(idx)
        } else if idx == self.plan.body.len() {
            Some(&self.plan.joint)
        } else {
            None
        }
    }

    /// Plan exhausted: the chunk has played, waiting for the next (the
    /// composer substitutes it). Until then we hold the last section, the music
    /// doesn't stop.
    pub fn plan_exhausted(&self) -> bool {
        self.plan_done
    }

    /// How many bars of the current chunk have played: from this the chunk is
    /// re-planned on a live seed/style change (the change takes effect from the
    /// next bar, what has played is left alone).
    pub fn piece_bar(&self) -> usize {
        self.total_measures.saturating_sub(self.piece_start_measure)
    }

    /// Current chunk number: from it the remainder is re-planned (a live
    /// seed/style change must not break the chunk's form).
    pub fn piece(&self) -> u64 {
        self.piece
    }

    /// Transition shadow: a copy of the arranger whose next item is the plan
    /// of the following chunk. A copy, not a new arranger: state (the visit
    /// counter, tonic, harmony) must match the main one, otherwise
    /// the shadow would sing a different chunk in a different key.
    pub fn shadow_of(&self, plan: crate::piece::PiecePlan) -> Self {
        let mut s = self.clone();
        s.plan = plan;
        s.plan_idx = 0;
        s.plan_done = false;
        s.next_piece = None;
        s.section_measure_count = 0;
        // The shadow's chunk loop is already the next one — like the main one after
        // receiving a plan.
        s.cycle = self.cycle + 1;
        s.enter_section();
        // The shadow sings the first bar of a new chunk right away: the section counter
        // starts at one, while the session-wide bar counter doesn't
        // shift — the shadow and the main one run on the same clock.
        s.section_measure_count = 1;
        s
    }

    /// Is a seam (chunk finale) in progress: during these bars the next chunk enters
    /// with a voice over the groove.
    pub fn in_joint(&self) -> bool {
        self.plan_idx >= self.plan.body.len()
    }

    /// Plan of the next chunk (written when the seam starts): from it the
    /// composer builds the "shadow" — the next chunk's voice.
    pub fn next_plan(&self) -> Option<&crate::piece::PiecePlan> {
        self.next_piece.as_ref()
    }

    /// How far the seam has progressed (0..1): the shadow's volume grows toward its end.
    pub fn joint_progress(&self) -> f32 {
        if self.section_len == 0 {
            return 0.0;
        }
        (self.section_measure_count as f32 / self.section_len as f32).clamp(0.0, 1.0)
    }

    /// Seam progress by steps (0..1): the shadow voice enters not at a bar boundary but
    /// smoothly, step by step — the entrance is heard as an event, not as a hit.
    pub fn joint_step_progress(&self, steps_per_measure: usize, step_in_measure: usize) -> f32 {
        if self.section_len == 0 {
            return 0.0;
        }
        let steps = self
            .section_measure_count
            .saturating_sub(1)
            .saturating_mul(steps_per_measure)
            .saturating_add(step_in_measure) as f32;
        let total = (self.section_len * steps_per_measure).max(1) as f32;
        (steps / total).clamp(0.0, 1.0)
    }

    /// Substitute a chunk plan: the re-planned remainder of the current one (a live
    /// seed/style change takes effect from the next bar).
    pub fn set_plan(&mut self, plan: crate::piece::PiecePlan) {
        self.plan = plan;
        self.plan_idx = 0;
        self.plan_done = false;
        self.next_piece = None;
        self.piece_start_measure = self.total_measures;
        self.section_measure_count = 0;
        self.enter_section();
    }

    /// Next chunk of the stream: the form is written by itself when the seam of the
    /// current one starts. The join is written in advance — the next chunk is known while
    /// the current one's finale is playing.
    fn plan_next_piece(&mut self) {
        let index = self.piece + 1;
        self.next_piece = Some(crate::piece::plan_piece(
            self.seed,
            self.bpm,
            self.score.cycle,
            index,
            0,
            self.transition,
            self.layers,
        ));
    }

    /// Entering a section: the length and the monotonic salt of the visit.
    fn enter_section(&mut self) {
        // The seam (chunk finale) is the moment when the next chunk is written:
        // the join is written in advance, not stitched on the fly.
        if self.plan_idx == self.plan.body.len() && self.next_piece.is_none() {
            self.plan_next_piece();
        }
        let planned = self.plan_section(self.plan_idx).map(|sp| {
            (
                sp.section,
                sp.energy,
                sp.layers,
                if sp.vary {
                    varied_len(sp.section, sp.bars, self.seed_hash, self.stages_done)
                } else {
                    sp.bars
                },
            )
        });
        match planned {
            Some((section, energy, layers, len)) => {
                // The old section becomes the previous one exactly on entry.
                // Needed for transitions and the poles crossfade, not for history.
                self.previous_section = self.section;
                self.section = section;
                self.section_energy = energy;
                self.section_layers = layers;
                self.section_len = len;
            }
            None => {
                // Plan exhausted: hold the current section (it has already played),
                // until the plan is substituted.
                self.plan_done = true;
                self.section_len = self.section_len.max(1);
            }
        }
        self.stages_done += 1;
        self.section_measure_count = 0;
    }

    fn section(&self) -> SongSection {
        self.section
    }

    /// Extend the tonic walk to visit pair `pair` (inclusive —
    /// more precisely, add the steps `tonic_done..pair`). Pairs grow monotonically
    /// together with the scenes, so each step is computed once
    /// over the whole endless session: O(1) per bar instead of O(stage).
    fn extend_tonic(&mut self, pair: usize) -> i64 {
        while self.tonic_done < pair {
            self.tonic_pos += drift_step(self.seed_hash, self.tonic_done);
            self.tonic_done += 1;
        }
        self.tonic_pos
    }

    pub fn advance_measure(&mut self) -> SongContext {
        self.section_measure_count += 1;
        self.total_measures += 1;

        if self.section_measure_count > self.section_len {
            // The section ended. The seam has played — the chunk is exhausted: take
            // the next chunk (its plan was written when the seam started).
            if self.plan_idx + 1 > self.plan.body.len() {
                if let Some(next) = self.next_piece.take() {
                    let overlap = self.transition.overlap_bars();
                    self.plan = next;
                    self.piece += 1;
                    self.cycle += 1;
                    self.plan_idx = 0;
                    // Transition overhang: the first bars of the new chunk were already
                    // sung by the shadow voice (it entered in the seam). We continue from
                    // that same point, the section's length stays full — otherwise
                    // the energy would jump at the join.
                    self.piece_start_measure = self.total_measures.saturating_sub(overlap);
                    // enter_section resets the counter — the current bar is the first.
                    self.enter_section();
                    self.section_measure_count = overlap + 1;
                    self.section_len = self.section_len.max(overlap + 1);
                } else {
                    // No plan (the form must not stop): hold the section.
                    self.plan_done = true;
                    self.section_len = self.section_len.max(1);
                    self.section_measure_count = 1;
                    // A held section must not look "just entered" every bar:
                    // otherwise the tail of the BuildUp transition would repeat
                    // in every bar of the Drop.
                    self.previous_section = self.section;
                }
            } else {
                self.plan_idx += 1;
                self.enter_section();
                // enter_section reset the counter — the current bar is the first.
                self.section_measure_count = 1;
            }
        }

        self.build_context()
    }

    /// Context of the current bar (without advancing counters). Separate from
    /// `advance_measure`, so that the transition shadow reads the same bar as the
    /// main arranger without shifting the session counter.
    pub fn build_context(&mut self) -> SongContext {
        let section = self.section();
        let is_last = self.section_measure_count == self.section_len;
        let progress = self.section_measure_count as f32 / self.section_len as f32;
        // Phrases are 2 bars, boundaries on odd bars (1,3,5,...):
        // sections of active styles have even lengths and start on an odd
        // bar, so a phrase never breaks at a section join.
        let phrase_index = (self.total_measures - 1) / PHRASE_LENGTH_MEASURES
            + self.phrase_offset
            + (self.cycle as usize).wrapping_mul(CYCLE_PHRASE_JUMP);
        // Volume glissando: halfway to the table over a bar. Section joins
        // flow rather than click — nobody fades out or bursts in suddenly.
        // The first bar is already at the target (prev_gains initialized from the table).
        let target = stage_gains_for(section);
        let mut gains = self.prev_gains;
        for (g, t) in gains.iter_mut().zip(target) {
            *g += (t - *g) * 0.5;
        }
        self.prev_gains = gains;

        // The tonic stands in visit pairs (see extend_tonic): incrementally,
        // once per session, not recalculated from scratch each step.
        let stage = self.stages_done as usize;
        let tonic_walk = self.extend_tonic(stage / 2);
        // A new chunk — a new B variation and a new chord order;
        // the first cycle keeps the seed's written foundation. The seam's shadow computes
        // the same cycle, so the next chunk is sung in the same key.
        let (motif_b, prog) = remix_for_cycle(self.motif_b, self.prog, self.seed_hash, self.cycle);

        SongContext {
            section,
            // Session-wide bar: the harmony, the riff cell and the second
            // half of the motif live by it, not by a local counter.
            measure_index: self.total_measures,
            phrase_index,
            energy: energy_for(
                section,
                progress,
                self.cycle,
                self.seed_hash,
                self.stages_done,
                self.section_energy,
            ),
            progress,
            is_last_measure: is_last,
            section_measure_index: self.section_measure_count,
            previous_section: self.previous_section,
            stage,
            cycle: self.cycle,
            stage_gains: gains,
            layers: self.section_layers,
            seed_hash: self.seed_hash,
            density: self.density,
            motif: self.motif,
            motif_b,
            // The section's theme: climaxes (Drop/PostChorus) sing B,
            // the rest — A. Energy and theme reinforce each other:
            // a real chorus, not the same melody louder.
            use_alt_motif: matches!(section, SongSection::Drop | SongSection::PostChorus),
            prog,
            tonic_walk,
        }
    }
}

/// Form filter openness 0..1: the engine multiplies the high-cut by it.
/// The theme holds the middle, the build sweeps upward through the whole build-up and opens
/// at the drop, the breakdown closes back down. Pure function.
pub fn section_openness(section: SongSection, progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    match section {
        SongSection::MainMotif => 0.4,
        SongSection::BuildUp => 0.4 + 0.6 * p,
        SongSection::Drop => 1.0,
        SongSection::PostChorus => 0.3,
    }
}

fn energy_for(
    section: SongSection,
    progress: f32,
    cycle: u64,
    seed_hash: u64,
    salt: u64,
    written: Option<f32>,
) -> f32 {
    // Written section energy (from the chunk plan) — if present, we take it
    // as the basis; otherwise the general form formula.
    let base = match written {
        Some(b) => b,
        None => match section {
            SongSection::MainMotif => 0.55,
            SongSection::BuildUp => 0.6,
            SongSection::Drop => return 1.0,
            SongSection::PostChorus => 0.45,
        },
    };
    // The drop is the anchor, always exactly 1.0.
    if section == SongSection::Drop {
        return 1.0;
    }
    // The build-up rises, the breakdown settles: energy is the section's motion.
    let ramp = match section {
        SongSection::BuildUp => progress * 0.4,
        SongSection::PostChorus => -progress * 0.1,
        _ => 0.0,
    };
    // Slight energy breathing across loops: the same form, a bit different intensity.
    let wobble = (hash01(seed_hash, cycle, salt) - 0.5) * 0.15;
    (base + ramp + wobble).clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_continues_the_next_piece_without_a_seam() {
        // Overlapping transition: the shadow sings the first bars of the next chunk,
        // and the main arranger continues that same chunk from the same place.
        // Section, progression and key must match — otherwise
        // the join is heard as a key change.
        use crate::piece::{plan_piece, Transition};
        let transition = Transition::Rolling { bars: 2 };
        let plan = plan_piece(
            424242,
            126.0,
            ENERGETIC_SCORE.cycle,
            0,
            0,
            transition,
            crate::piece::default_layers(),
        );
        let mut a = Arranger::with_plan(
            424242,
            126.0,
            ENERGETIC_SCORE,
            plan,
            transition,
            crate::piece::default_layers(),
        );
        let mut shadow: Option<Arranger> = None;
        let mut shadow_ctx: Option<SongContext> = None;
        let mut was_joint = false;
        let mut checked = false;
        for _ in 0..96 {
            let ctx = a.advance_measure();
            // The shadow keeps in step with the main one: the same bars.
            if let Some(s) = shadow.as_mut() {
                shadow_ctx = Some(s.advance_measure());
            }
            if a.in_joint() {
                if shadow.is_none() {
                    let next = a.next_plan().expect("plan of the next piece").clone();
                    let mut s = a.shadow_of(next);
                    let song = s.build_context();
                    shadow = Some(s);
                    shadow_ctx = Some(song);
                }
            } else {
                if was_joint {
                    // The main arranger took the next chunk: the shadow must
                    // stand at exactly the same bar.
                    let sc = shadow_ctx.expect("shadow context");
                    assert_eq!(ctx.section, sc.section, "sections diverged");
                    assert!(
                        (ctx.progress - sc.progress).abs() < 0.01,
                        "progression diverged: {} vs {}",
                        ctx.progress,
                        sc.progress
                    );
                    assert_eq!(ctx.tonic_walk, sc.tonic_walk, "tonic diverged");
                    assert_eq!(ctx.motif, sc.motif, "motif diverged");
                    checked = true;
                }
                shadow = None;
                shadow_ctx = None;
            }
            was_joint = a.in_joint();
        }
        assert!(checked, "join not checked: the seam hasn't arrived");
    }

    #[test]
    fn remix_for_cycle_variants_keep_hook_and_harmony() {
        // Chunks differ by the B variation and chord order, but the hook,
        // the cadence and the set of progression degrees stay the seed's foundation.
        let seed_hash = splitmix64(42 ^ 0x51ED);
        let motif_b = [2u8, 3, 2, 1, 2, 3, 3, 4, 3, 2, 3, 4, 3, 2, 1, 0];
        let prog = [0u8, 2, 3, 1];
        assert_eq!(remix_for_cycle(motif_b, prog, seed_hash, 0), (motif_b, prog));
        let mut variants = std::collections::HashSet::new();
        for cycle in 1..8u64 {
            let (b, p) = remix_for_cycle(motif_b, prog, seed_hash, cycle);
            assert_eq!(&b[..4], &motif_b[..4], "hook changed");
            assert_eq!(&b[12..], &motif_b[12..], "cadence changed");
            let mut sorted = p;
            sorted.sort_unstable();
            let mut base_sorted = prog;
            base_sorted.sort_unstable();
            assert_eq!(sorted, base_sorted, "harmony foundation changed");
            variants.insert((b, p));
        }
        assert!(variants.len() >= 2, "pieces share one remix: {variants:?}");
        assert_eq!(
            remix_for_cycle(motif_b, prog, seed_hash, 3),
            remix_for_cycle(motif_b, prog, seed_hash, 3),
            "remix not deterministic"
        );
    }

    #[test]
    fn filter_opens_toward_drop() {
        // Form sweep: the build reaches 1.0, the drop is open,
        // the breakdown is below the theme.
        assert_eq!(section_openness(SongSection::BuildUp, 1.0), 1.0);
        assert_eq!(section_openness(SongSection::Drop, 0.5), 1.0);
        assert!(
            section_openness(SongSection::PostChorus, 0.0)
                < section_openness(SongSection::MainMotif, 0.0)
        );
    }

    #[test]
    fn full_cycle_visits_every_section_no_outro() {
        // Active loop: theme, build-up, drop, breakdown — and no
        // finales or build-ups at the start: generation is endless and immediately at full strength.
        let mut a = Arranger::new(1);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..96 {
            seen.insert(a.advance_measure().section);
        }
        for s in [
            SongSection::MainMotif,
            SongSection::BuildUp,
            SongSection::Drop,
            SongSection::PostChorus,
        ] {
            assert!(seen.contains(&s), "section {s:?} never visited");
        }
    }

    #[test]
    fn stages_follow_the_score() {
        // One pass of the loop: theme → build-up → drop → breakdown.
        // No build-up: the first visit is already the theme.
        let mut a = Arranger::new(1);
        let mut order = Vec::new();
        let mut last = SongSection::PostChorus;
        let mut first = true;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if first || ctx.section != last {
                order.push(ctx.section);
                last = ctx.section;
                first = false;
            }
        }
        use SongSection::*;
        assert_eq!(
            order,
            vec![MainMotif, BuildUp, Drop, PostChorus],
            "{order:?}"
        );
    }

    #[test]
    fn calm_score_breathes_main_with_rare_variety() {
        // A calm chunk breathes with the theme but still goes through the arc:
        // theme, build-up, hit (timbre change), breakdown and return to the theme.
        let mut a = Arranger::with_score(1, CALM_SCORE);
        let mut order = Vec::new();
        let mut last = SongSection::Drop;
        let mut first = true;
        for _ in 0..100 {
            let ctx = a.advance_measure();
            if first || ctx.section != last {
                order.push(ctx.section);
                last = ctx.section;
                first = false;
            }
        }
        use SongSection::*;
        // A chunk: theme, build-up, hit, breakdown, theme return.
        assert_eq!(
            &order[..4],
            &[MainMotif, BuildUp, Drop, PostChorus],
            "{order:?}"
        );
        assert_eq!(order[4], MainMotif, "theme didn't return: {order:?}");
        let mains = order.iter().filter(|s| **s == MainMotif).count();
        let builds = order.iter().filter(|s| **s == BuildUp).count();
        assert!(mains >= builds, "calm must breathe main: {order:?}");
    }

    #[test]
    fn pieces_run_one_after_another() {
        // The chunk is finite: body, seam-finale, then the next chunk.
        // The stream is endless, but the form is a chain of chunks, not a loop.
        let mut a = Arranger::new(3);
        let mut cycles = std::collections::HashSet::new();
        let mut last = None;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if last != Some(ctx.section) {
                cycles.insert(ctx.cycle);
                last = Some(ctx.section);
            }
        }
        assert!(
            cycles.len() >= 3,
            "chunks don't replace each other: {:?}",
            cycles
        );
    }

    #[test]
    fn main_gains_stable_across_visits() {
        // Without reprises: the settled balance of the theme is the same in all visits.
        // We compare the last bars of visits (the glissando has already arrived).
        let mut a = Arranger::new(1);
        let mut mains = Vec::new();
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if ctx.section == SongSection::MainMotif && ctx.is_last_measure {
                mains.push(ctx.stage_gains);
            }
        }
        assert!(!mains.is_empty(), "no mains visited");
        for gains in &mains[1..] {
            assert_eq!(*gains, mains[0], "main balance drifted");
        }
    }

    #[test]
    fn groove_continues_through_postchorus() {
        // The breakdown isn't a restart: the rhythm section rolls on,
        // only the top rests (lead).
        let mut a = Arranger::new(1);
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            if ctx.section == SongSection::PostChorus && ctx.is_last_measure {
                // Settled breakdown (glissando arrived): the rhythm rolls,
                // the top rests. The first bar is transitional — not measured.
                let g = ctx.stage_gains;
                let kick = InstrumentRole::Kick.index();
                let hat = InstrumentRole::Hat.index();
                let lead = InstrumentRole::Lead.index();
                assert!(g[kick] >= 0.8, "kick stopped in post: {}", g[kick]);
                assert!(g[hat] >= 0.8, "hat stopped in post: {}", g[hat]);
                assert!(g[lead] < 0.7, "lead did not rest: {}", g[lead]);
                return;
            }
        }
        panic!("postchorus never visited");
    }

    #[test]
    fn postchorus_is_a_breather() {
        // The breakdown is quieter than the drop: the loop breathes, not hammers.
        let mut a = Arranger::new(1);
        let mut drop_sum = None;
        let mut post_sum = None;
        for _ in 0..200 {
            let ctx = a.advance_measure();
            if ctx.cycle > 0 {
                break;
            }
            let sum: f32 = ctx.stage_gains[..7].iter().sum();
            match ctx.section {
                SongSection::Drop if drop_sum.is_none() => {
                    drop_sum = Some(sum);
                }
                SongSection::PostChorus if post_sum.is_none() => {
                    post_sum = Some(sum);
                }
                _ => {}
            }
        }
        assert!(
            post_sum.unwrap() < drop_sum.unwrap(),
            "postchorus must dip after drop"
        );
    }

    #[test]
    fn same_seed_same_song() {
        let mut a = Arranger::new(777);
        let mut b = Arranger::new(777);
        for _ in 0..20 {
            let x = a.advance_measure();
            let y = b.advance_measure();
            assert_eq!(x.phrase_index, y.phrase_index);
            assert_eq!(x.energy, y.energy);
            assert_eq!(x.cycle, y.cycle);
            assert_eq!(x.motif, y.motif);
            assert_eq!(x.motif_b, y.motif_b);
            assert_eq!(x.use_alt_motif, y.use_alt_motif);
        }
    }

    #[test]
    fn phrases_span_two_measures_from_the_first_one() {
        // The phrase is exactly 2 bars, the first phrase starts at the first bar.
        // Bar within the phrase = (measure_index - 1) % 2.
        let mut a = Arranger::new(1);
        let first = a.advance_measure();
        let second = a.advance_measure();
        let third = a.advance_measure();
        let fourth = a.advance_measure();
        assert_eq!(first.measure_index, 1);
        assert_eq!(second.measure_index, 2);
        assert_eq!(third.measure_index, 3);
        assert_eq!(fourth.measure_index, 4);
        assert_eq!(first.phrase_index, second.phrase_index);
        assert_eq!(third.phrase_index, fourth.phrase_index);
        assert_ne!(first.phrase_index, third.phrase_index);
    }

    #[test]
    fn measure_index_is_global_across_sections() {
        // section_measure_count resets at joins, the absolute bar doesn't:
        // the progression and the riff cell move by it.
        let mut a = Arranger::new(1);
        let mut measures = 0usize;
        let mut last_section = None;
        for _ in 0..300 {
            let ctx = a.advance_measure();
            measures += 1;
            assert_eq!(
                ctx.measure_index, measures,
                "measure index must be global and monotonic"
            );
            if let Some(prev) = last_section {
                if prev != ctx.section && ctx.measure_index == 1 {
                    panic!("measure index reset on section boundary");
                }
            }
            last_section = Some(ctx.section);
        }
    }

    #[test]
    fn phrases_never_straddle_section_boundaries() {
        // Sections have even lengths and start on an odd bar: the phrase
        // (2 bars) lies wholly inside a section, not broken at a join.
        let mut a = Arranger::new(9);
        let mut last_section = None;
        let mut last_phrase = usize::MAX;
        for _ in 0..400 {
            let ctx = a.advance_measure();
            if let Some(prev) = last_section {
                if prev != ctx.section {
                    let bar = (ctx.measure_index - 1) % 2;
                    assert_eq!(bar, 0, "section starts mid-phrase: {ctx:?}");
                    assert_ne!(
                        ctx.phrase_index, last_phrase,
                        "new section must start a new phrase"
                    );
                }
            }
            last_section = Some(ctx.section);
            last_phrase = ctx.phrase_index;
        }
    }

    #[test]
    fn alt_motif_marks_drop_sections() {
        // B — the climax material: Drop and PostChorus sing B, the rest A.
        // Verse/chorus differ by theme, not only by volume.
        for seed in [0u64, 1, 7, 42, 777, 424242] {
            let mut a = Arranger::new(seed);
            let first = a.advance_measure();
            assert_ne!(first.motif, first.motif_b, "seed {seed}: A == B");
            assert!(!first.use_alt_motif, "starts with A");
            // The theme's range is the seed's handwriting: narrow stepwise (≤1),
            // wide third-wise (≤2). The cadence is always on the tonic.
            let seed_hash = splitmix64(seed ^ 0x51ED);
            let lim: i8 = if splitmix64(seed_hash ^ 0x1DE5).is_multiple_of(2) {
                2
            } else {
                1
            };
            for (name, m) in [("motif", first.motif), ("motif_b", first.motif_b)] {
                for v in m {
                    assert!(v <= 4, "{name} out of range: {v}");
                }
                for w in m.windows(2) {
                    assert!(
                        (w[0] as i8 - w[1] as i8).abs() <= lim,
                        "{name} leaps: {m:?}"
                    );
                }
                assert_eq!(m[15], 0, "{name} without cadence: {m:?}");
            }
        }
        let mut a = Arranger::new(5);
        let (mut main_a, mut drop_b, mut post_b) = (false, false, false);
        for _ in 0..200 {
            let ctx = a.advance_measure();
            match ctx.section {
                SongSection::Drop => {
                    assert!(ctx.use_alt_motif, "drop must sing B");
                    drop_b = true;
                }
                SongSection::PostChorus => {
                    assert!(ctx.use_alt_motif, "postchorus must sing B");
                    post_b = true;
                }
                _ => {
                    assert!(!ctx.use_alt_motif, "{:?} must sing A", ctx.section);
                    if ctx.section == SongSection::MainMotif {
                        main_a = true;
                    }
                }
            }
            if main_a && drop_b && post_b {
                break;
            }
        }
        assert!(
            main_a && drop_b && post_b,
            "sections never complete a cycle"
        );
        // Boundaries are determined by the seed.
        let mut x = Arranger::new(5);
        let mut y = Arranger::new(5);
        for _ in 0..40 {
            assert_eq!(
                x.advance_measure().use_alt_motif,
                y.advance_measure().use_alt_motif,
                "theme boundary must be deterministic"
            );
        }
    }

    #[test]
    fn drop_has_full_energy() {
        let mut a = Arranger::new(0);
        let mut found_drop = false;
        for _ in 0..96 {
            let ctx = a.advance_measure();
            if ctx.section == SongSection::Drop {
                assert_eq!(ctx.energy, 1.0);
                found_drop = true;
            }
        }
        assert!(found_drop);
    }

    #[test]
    fn cycles_advance_and_vary_lengths() {
        // Loops move forward, but the theme lengths between loops differ:
        // the form doesn't repeat verbatim.
        let mut a = Arranger::new(1);
        let mut max_cycle = 0;
        let mut main_lens = std::collections::HashSet::new();
        let mut current_main = 0;
        let mut in_main = false;
        for _ in 0..300 {
            let ctx = a.advance_measure();
            max_cycle = max_cycle.max(ctx.cycle);
            if ctx.section == SongSection::MainMotif {
                if !in_main {
                    if current_main > 0 {
                        main_lens.insert(current_main);
                    }
                    current_main = 0;
                    in_main = true;
                }
                current_main += 1;
            } else {
                in_main = false;
            }
        }
        assert!(max_cycle >= 2, "cycles do not advance: {max_cycle}");
        assert!(
            main_lens.len() > 1,
            "main length never varies: {main_lens:?}"
        );
    }

    #[test]
    fn starts_at_full_strength() {
        // No build-up: the first bar is already the theme at full strength,
        // all roles are audible (gains > 0.3), the kick drum's heartbeat ticks.
        let mut a = Arranger::new(3);
        let ctx = a.advance_measure();
        assert_eq!(ctx.section, SongSection::MainMotif);
        assert!(!ctx.use_alt_motif, "starts with A");
        for (i, g) in ctx.stage_gains[..7].iter().enumerate() {
            assert!(*g > 0.3, "role {i} muted at start: {g}");
        }
    }

    #[test]
    fn density_stays_in_range() {
        for seed in 0..20 {
            let d = Arranger::new(seed).advance_measure().density;
            assert!(
                (0.7..=1.3).contains(&d),
                "seed {seed}: density {d} out of range"
            );
        }
    }

    #[test]
    fn progressions_differ_between_seeds() {
        // Harmonic motion — from the seed: different seeds lead
        // the accompaniment differently with a recognizable motif.
        let progs: std::collections::HashSet<_> = (0..16u64)
            .map(|s| Arranger::new(s).advance_measure().prog)
            .collect();
        assert!(progs.len() > 1, "progression never varies");
    }

    #[test]
    fn same_seed_same_progression() {
        // The progression is stable within a seed — harmony doesn't jump.
        let a = Arranger::new(4242).advance_measure().prog;
        let b = Arranger::new(4242).advance_measure().prog;
        assert_eq!(a, b);
        for step in a {
            assert!(step <= 4, "prog step out of scale range: {step}");
        }
    }

    #[test]
    fn motifs_differ_between_seeds() {
        let motifs: std::collections::HashSet<_> = (0..16u64)
            .map(|s| Arranger::new(s).advance_measure().motif)
            .collect();
        assert!(motifs.len() > 3, "motifs do not vary");
    }
}
