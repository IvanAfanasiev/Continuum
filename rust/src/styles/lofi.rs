use super::riff::RiffKind;
use super::transition::{Kind, Spec};
use super::{Melody, RoleGuide, Style, StyleGroup, Voicing};
use crate::instruments::Slot;

/// Lo-fi: dusty, warm, crooked. Boom-bap anchors (kick on 1 and syncopation,
/// snare on 2 and 4, deep bass, Rhodes) + vinyl crackle.
/// No EDM leads, zero quantise and explosive drops: energy
/// flattened, swing strong, top cut off.
/// Soft minor without sixth degree: pentatonic scale plus ninth - jazz
/// an open top pad rather than a blues cell. The sixth doesn't sound
/// deliberately: on it, lo-fi loses its lazy uncertainty.
const SCALE: [u8; 6] = [48, 50, 51, 53, 55, 58];

/// Lo-fi moves: ii–IV–i and smooth sequences without sudden jumps.
const PROGS: [[u8; 4]; 6] = [
    [0, 3, 4, 5],
    [0, 4, 3, 5],
    [3, 5, 0, 4],
    [0, 2, 4, 3],
    [5, 3, 0, 4],
    [0, 5, 4, 3],
];
/// Lo-fi is in no hurry: the chord lasts 4-8 bars.
const HARMONY_BARS: [usize; 2] = [4, 8];

const KICK_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.35, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const SNARE_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.15,
];
const HAT_W: &[f32] = &[
    // Dusty ticks: emphasis on offbeats, even beats are quieter.
    // It was the other way around (1.0 on each beat) - a bright metal hat
    // dominated the mix (stem audit: most high-frequency voice).
    0.3, 0.0, 0.9, 0.0, 0.3, 0.0, 0.9, 0.0, 0.3, 0.0, 0.9, 0.0, 0.3, 0.0, 0.9, 0.4,
];
const BASS_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const PAD_W: &[f32] = &[
    // Pad - foundation: anchor on downbeat (breathes priority 0.95),
    // not a rare guest. Without a drone, lofi falls apart into dots.
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const KEYS_W: &[f32] = &[
    // Rhodes comping: downbeat anchor + stabs inside the bar.
    // Two hits per bar gave “once every 3 seconds with a break” -
    // We keep dustiness with the rarity of phrases, and not with the emptiness of the bar.
    1.0, 0.0, 0.0, 0.45, 0.0, 0.0, 0.5, 0.0, 0.6, 0.0, 0.4, 0.0, 0.0, 0.0, 0.45, 0.0,
];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Long guitar over Rhodes: rare notes with breath.
// The strum base is even eighths (ref: Coming Home); downstrokes on beats —
// anchors (the strum is always audible), upstroke fills.
const GUITAR_STRUM8_W: &[f32] = &[
    1.0, 0.0, 0.65, 0.0, 1.0, 0.0, 0.65, 0.0, 1.0, 0.0, 0.65, 0.0, 1.0, 0.0, 0.65, 0.4,
];
// Second boom-bap: syncopation went from 7 to 6, response from 10 to 12.
const KICK_BOOM_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.35, 0.0, 0.2, 0.0, 0.0, 0.0,
];
// Half-time: kick drum for 1 and 2, air instead of fuss.
const KICK_HALF_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Syncopated funk kick (ref: Redbone - no straight kick).
// Sparse and anchored at 1: boom-bap, not machine gun.
// (7 common candidates gave mush at high energy/intensity.)
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.6, 0.0, 0.0, 0.0, 0.4, 0.0,
];
const KICK_POLES: &[&[f32]] = &[KICK_W, KICK_BOOM_W, KICK_SYNCOP_W, KICK_HALF_W];
const HAT_EVEN_W: &[f32] = &[
    0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45, 0.7, 0.45,
];
// Sparse offbeats (ref: Snowman, Redbone - only weak beats,
// no sixteenth ticks).
const HAT_SPARSE_W: &[f32] = &[
    0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.9, 0.4,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_EVEN_W, HAT_SPARSE_W];
// Shaker: dusty offbeats.
const SHAKER_LOFI_W: &[f32] = &[
    0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0,
];

pub struct LofiStyle {}

impl LofiStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for LofiStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for LofiStyle {
    fn name(&self) -> &str {
        "lofi"
    }

    fn group(&self) -> StyleGroup {
        StyleGroup::Chill
    }

    fn bpm(&self) -> f32 {
        78.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Heavy reference swing (Coming Home 1.69, Autumn ~1.5):
        // the combat swings, not ticks.
        1.55
    }

    fn scale(&self) -> &'static [u8] {
        &SCALE
    }

    fn progressions(&self) -> &'static [[u8; 4]] {
        &PROGS
    }

    fn harmonic_rhythm(&self) -> &'static [usize] {
        &HARMONY_BARS
    }

    fn key_shift(&self) -> i8 {
        // D minor: a tone higher than club C, softer in register.
        2
    }

    fn density(&self) -> f32 {
        0.78
    }

    /// Lo-fi sings lazily: syncopations instead of a smooth grid, the notes drag on,
    /// the downbeats gently reach out to the chord. First cell of the theme
    /// returns - the hook is heard, and does not dissolve in variations.
    fn melody(&self) -> Melody {
        Melody {
            onsets: super::phrase_mask(0b1001_0010_1001_0110),
            chord_snap: 0.6,
            sustain: 1.3,
            cells: [0, 2, 0, 3],
            phrased: true,
        }
    }

    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::CALM_SCORE
    }

    fn arrangement_for_seed(&self, seed: u64) -> crate::arranger::Score {
        use crate::arranger::{Score, SongSection};
        const BREATH: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 10),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 6),
                (SongSection::PostChorus, 4),
                (SongSection::MainMotif, 4),
            ],
        };
        const AFTERGLOW: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 6),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 8),
                (SongSection::PostChorus, 4),
                (SongSection::MainMotif, 6),
            ],
        };
        crate::arranger::pick_score(
            seed,
            0x10F1_CA1F,
            &[crate::arranger::CALM_SCORE, BREATH, AFTERGLOW],
        )
    }

    /// Arrange lo-fi: pad and Rhodes hold the theme (the theme should not be empty),
    /// vinyl and rain - smooth background
    /// post-chorus. A lo-fi drop is not a hit, but a change in timbre.
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{default_layers, L_GROOVE, L_HARMONY, L_TEXTURE, L_THEME};
        let base = default_layers();
        [
            // Theme: groove, bass, pump, theme and texture.
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Build-up: the theme develops.
            base[1],
            // Drop: the same composition as the theme — lofi has no “hit”.
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Breathing: groove and texture, the voice rests.
            L_GROOVE | L_TEXTURE,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Quiet junction: the piece closes and the next one starts clean
        // measure. A crossfade here would sound like stuttering.
        crate::piece::Transition::Cut
    }

    /// Lofi doesn't hit, but exhales: a quiet pickup hat at the end of build-up and
    /// two steps of dust after it. No roll - this is not a club.
    fn transition_gesture(&self) -> Option<Spec> {
        Some(Spec::new(
            Kind::BreathPickup,
            &[Slot::Hat],
            &[Slot::Hat],
            2,
            42,
        ))
    }

    fn riff_kind(&self) -> RiffKind {
        RiffKind::Lofi
    }

    fn reverb_wet(&self) -> f32 {
        0.12
    }

    fn reverb_size(&self) -> f32 {
        0.35
    }

    fn treble_cutoff(&self) -> f32 {
        // Metering against references (Autumn 1557, Snowman 824, Redbone 798):
        // ours was 2504 - the brightest. Darken towards the middle of the references.
        5200.0
    }

    /// Dusty shelf: energy 0.32..0.70 - no explosions, no failures.
    fn shape_energy(&self, energy: f32) -> f32 {
        0.32 + 0.38 * energy
    }

    /// The sidechain is barely breathing: lofi pumps softly, without EDM pump.
    fn sidechain_depth(&self) -> f32 {
        0.5
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Classics: drums + bass + Rhodes + guitar + pad + vinyl +
        // The guitar is the signature of the style: it’s plucked and Rhodes sings
        // one theme in different voices (with a shifted echo grid).
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Pad,
            Slot::Snare,
            Slot::Hat,
            Slot::Keys,
            Slot::Vinyl,
            Slot::Guitar,
            Slot::Piano,
        ]
    }

    fn signature_slots(&self) -> &'static [Slot] {
        // Rhodes and guitar picking - the face of lofi; The pad holds the harmony.
        &[Slot::Keys, Slot::Guitar]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // Sub-carpet: soft and long, without sharp attacks. Quieter than the drive -
        // the emphasis is on the guitar and keys, the bass only holds the bottom.
        // Legato instead of smacking: long notes (6 steps) + partial
        // sub-skew versus presence bounce. Heat left (0.4,
        // not 1.0) - a dull sine instead of bass is not needed.
        // Level below the guitar: spectrum measurement (Coming Home) - we have a share
        // bottom 0.38 versus 0.25 for the reference.
        // Dusty bass moves with roots (jitter 1 - adjacent stages).
        let bass = RoleGuide {
            energy_response: 0.8,
            gates: &[4.0, 6.0, 8.0],
            tie_repeats: 0.35,
            vel_base: 0.5,
            vel_jitter: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 1,
            },
            vel_high_cut: 0.6,
            sub_bias: 0.4,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, &[4.0, 6.0, 8.0], 0.5)
        };
        let kick = RoleGuide {
            energy_response: 0.8,
            vel_base: 0.9,
            vel_jitter: 0.1,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(KICK_W, 36, 0.9)
        };
        let pad = RoleGuide {
            gates: &[6.0, 8.0, 10.0],
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 0,
                jitter: 0,
            },
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, &[6.0, 8.0, 10.0], 0.35)
        };
        let snare = RoleGuide {
            vel_base: 0.7,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SNARE_W, 38, 0.7)
        };
        let hat = RoleGuide {
            long_offbeats: true,
            gates: &[0.3, 0.6, 1.1],
            vel_base: 0.34,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            poles: Some(HAT_POLES),
            // Hat holds the grid of poles: the pulse, instead of the grid, knocked down the rhythm.
            ..RoleGuide::kit(HAT_W, 42, 0.34)
        };
        // Guitar cuts: the same strum plan as the guitar (shared skeleton
        // roles), its timbre and its gate roll. Heterophony.
        let pluck = RoleGuide {
            gates: &[1.5, 2.0, 3.0],
            vel_base: 0.5,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.2,
            ..RoleGuide::pitched(GUITAR_STRUM8_W, 0, 1, 0, &[1.5, 2.0, 3.0], 0.5)
        };
        // Rhodes - lofi harmony: comping with wandering under the melody
        // guitars. Does not duplicate the theme: two carriers shared the density and
        // took the chord layer below the contract.
        let keys = RoleGuide {
            gates: &[4.0, 6.0, 8.0],
            vel_base: 0.6,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(KEYS_W, 0, 1, 2, &[4.0, 6.0, 8.0], 0.6)
        };
        // Alien slots: muted working guides - turned on
        // only with the front handle, the default character is not broken.
        // Guest-808 in lofi - sub-drop on 1, not a double kick pattern:
        // doubling gave flam and layering bottom.
        const SUB_DROP_W: &[f32] = &[
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let kick808 = RoleGuide {
            vel_base: 0.8,
            ..RoleGuide::kit(SUB_DROP_W, 36, 0.8)
        };
        // Dust is quieter than music: stem audit found vinyl to be the brightest
        // after the dark keys, we turn down the background, not the music.
        let vinyl = RoleGuide {
            gates: &[300.0],
            vel_base: 0.15,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[300.0], 0.15)
        };
        // Guitar - picking on the theme: one interval per hit, in a circle
        // triad. Together with Rhodes keeps the melody lofi, but its grid
        // shifted to an eighth note is an echo imitation, not a unison.
        const STRUM_IV: &[i8] = &[0, 2, 4];
        let guitar = RoleGuide {
            // Too much - lofi voice: it should not disappear in quiet
            // verses, so the energy response is higher than neutral.
            energy_response: 1.6,
            gates: &[1.5, 2.0, 3.0],
            vel_base: 0.85,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.2,
            chord: Some(STRUM_IV),
            arp: true,
            ..RoleGuide::pitched(GUITAR_STRUM8_W, 0, 1, 0, &[1.5, 2.0, 3.0], 0.85)
        };
        // Shaker outside club: dusty offbeats for the front handle.
        // Rain: an even veil in all phrases (quieter than vinyl and music).
        let rain = RoleGuide {
            gates: &[150.0],
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[150.0], 0.35)
        };
        let shaker = RoleGuide {
            energy_response: 1.0,
            gates: &[0.4],
            vel_base: 0.25,
            vel_jitter: 0.06,
            ..RoleGuide::kit(SHAKER_LOFI_W, 60, 0.25)
        };
        // Lead guitar outside the automix: quiet fingerpicking for the front knob.
        // The default lofi is not touched (it has its own fingerpicking guitar).
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.25,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 2,
            },
            ..RoleGuide::pitched(BED_W, 0, 1, 2, &[1.0, 1.5, 2.0], 0.25)
        };
        // Piano next to Rhodes: soft chord stabs, quieter than the theme.
        let piano = RoleGuide {
            energy_response: 0.8,
            gates: &[3.0, 4.0, 6.0],
            vel_base: 0.35,
            ..RoleGuide::pitched(KEYS_W, 0, 0, 1, &[3.0, 4.0, 6.0], 0.35)
        };
        [
            bass,
            kick,
            pad,
            snare,
            hat,
            pluck,
            keys,
            kick808,
            vinyl,
            guitar,
            rain,
            shaker,
            guitar_lead,
            piano,
        ]
    }
}
