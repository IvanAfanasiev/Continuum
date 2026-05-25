// A preset defines the musical style and theoretical constraints sent to the LLM.
// The two prompt fields are kept separate so they can be mixed independently
// for example: style = JAZZ, theory = MODAL.
#[derive(Debug, Clone)]
pub struct Preset {
    // Human-readable name shown in logs and future UI
    pub name: &'static str,
    // Character and sound description, tells the LLM *what* to generate
    pub style: &'static str,
    // Music-theory constraints, tells the LLM *how* to build the phrase
    pub theory: &'static str,
    // How many notes/groups to request per LLM call
    pub batch_size: usize,
    // Buffer low-water mark: the LLM is woken when len() drops below this.
    // Smaller = closer to real time, but higher risk of an audible gap.
    pub refill_threshold: usize,
}

impl Preset {
    // Build the system prompt that is sent to the LLM before each generation.
    pub fn system_prompt(&self) -> String {
        format!(
            "You are a music generator. Generate exactly {} musical events.\n\
             Format each event as: note|velocity|duration\n\
             Separate events with spaces. Example: 64|0.5|600 60|0.4|500\n\
             Style: {}\n\
             Rules: {}\n\
             Output ONLY the sequence of events, no other text.",
            self.batch_size, self.style, self.theory
        )
    }
}

// ─────────────────────────────────────────────────────────────
//  BUILT-IN PRESETS
// ─────────────────────────────────────────────────────────────

// Slow, sparse, contemplative ambient.
pub const AMBIENT: Preset = Preset {
    name: "Ambient",
    style: "slow, sparse, ethereal, lots of silence between notes, \
            long sustained tones, peaceful and meditative",
    theory: "ALLOWED MIDI NOTES ONLY: [60, 62, 64, 67, 69, 72, 74, 76]. \
             Strictly avoid any other notes. \
             Durations must be long: 800-2500ms. \
             Velocity must be quiet: 0.2-0.5.",
    batch_size: 16,
    refill_threshold: 4,
};

// Jazz: chromaticism, seventh chords, swing feel.
pub const JAZZ: Preset = Preset {
    name: "Jazz",
    style: "syncopated, chromatic, warm, improvisational, \
            bebop-inspired with a swing feel",
    theory: "use ii-V-I progressions, add 7th and 9th chord extensions, \
             chromatic passing tones allowed, notes 48-80, \
             mix short 200-400ms and medium 600-800ms durations, \
             vary velocity 0.3-0.8 for expression",
    batch_size: 28,
    refill_threshold: 6,
};

// Minimalism: repeating patterns with micro-variations.
pub const MINIMAL: Preset = Preset {
    name: "Minimal",
    style: "repetitive, hypnotic, slowly evolving, \
            Philip Glass or Steve Reich inspired",
    theory: "stay strictly in C major (C D E F G A B), \
             repeat short motifs of 3-4 notes with slight rhythmic variation, \
             notes 60-76, durations 250-500ms, \
             consistent velocity 0.4-0.6",
    batch_size: 48,
    refill_threshold: 8,
};

// Chaos: atonality and maximum unpredictability.
pub const CHAOS: Preset = Preset {
    name: "Chaos",
    style: "atonal, unpredictable, dissonant, experimental, \
            no musical rules, maximum surprise",
    theory: "use any MIDI notes 36-96 freely, \
             dissonant intervals encouraged (semitones, tritones), \
             wildly varying durations 100-1500ms, \
             extreme velocity swings 0.1-0.95",
    batch_size: 22,
    refill_threshold: 5,
};

// Classical: voice leading and functional harmony.
pub const CLASSICAL: Preset = Preset {
    name: "Classical",
    style: "elegant, structured, Bach or Mozart inspired, \
            clear melodic lines with harmonic accompaniment",
    theory: "use functional harmony I-IV-V-I in C major, \
             voice leading: avoid parallel fifths, \
             melody in range 60-79, bass 48-60, \
             mix quarter-note 400ms and half-note 800ms values, \
             velocity 0.4-0.7",
    batch_size: 26,
    refill_threshold: 6,
};

// Drone: long pedal tones with slow harmonic evolution.
pub const DRONE: Preset = Preset {
    name: "Drone",
    style: "sustained, meditative, slowly shifting harmonics, \
            dark and hypnotic, like Tibetan singing bowls",
    theory: "use a perfect-5th pedal tone (C3+G3 always present), \
             add slow melodic movement above in range 60-72, \
             very long durations 1500-3000ms, \
             low velocity 0.15-0.4, \
             use minor scale (C D Eb F G Ab Bb)",
    batch_size: 12,
    refill_threshold: 3,
};

// All presets as a slice — useful for UI pickers or random selection.
pub const ALL: &[&Preset] = &[
    &AMBIENT, &JAZZ, &MINIMAL, &CHAOS, &CLASSICAL, &DRONE,
];
