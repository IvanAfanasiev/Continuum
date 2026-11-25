use super::riff::RiffKind;
use super::transition::{Kind, Spec};
use super::{Melody, RoleGuide, Style, Voicing};
use crate::instruments::Slot;

/// Club: beatmaker. Straight kick + 808 layer, deep sub,
/// clap on 2 and 4 and vinyl background.
/// The straight 4/4 beat dominates: heavy and aggressive, without whistles.
const SCALE: [u8; 7] = [48, 50, 51, 53, 55, 56, 58];

/// Harmonic moves of the club: minor i–VI–iv–v and related ones.
/// The steps are indexed by SCALE; The seed chooses a move from the pool.
const PROGS: [[u8; 4]; 6] = [
    [0, 5, 3, 4],
    [0, 3, 5, 4],
    [0, 5, 6, 4],
    [5, 3, 0, 4],
    [0, 4, 5, 3],
    [3, 5, 0, 4],
];
/// The club changes chords every 2–4 measures: the harmony swings along with the beat.
const HARMONY_BARS: [usize; 2] = [2, 4];

const FOUR_FLOOR: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const CLAP_W: &[f32] = &[
    0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
];
const HAT_W: &[f32] = &[
    0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35, 0.0, 0.35, 1.0, 0.35,
];
const BASS_W: &[f32] = &[
    // Off-beat eighths instead of a wall of 16s: swing between kick drum hits,
    // not the carpet underneath. Wall stoked transients (crest 4.4 vs 7–12
    // from references) and darkened the mix.
    0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0,
];
const PAD_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
const LEAD_W: &[f32] = &[
    1.0, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3, 0.6, 0.3,
];
const STAB_W: &[f32] = &[
    0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0, 0.0, 0.0, 0.7, 0.0,
];
// Clap with ghost notes: the backbeat is held by anchors, between there is a barely audible whisper.
// Loud ghost notes gave a jumping snare instead of a backbeat.
const CLAP_GHOST_W: &[f32] = &[
    0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.15, 0.0, 1.0, 0.0, 0.0, 0.2,
];
const SNARE_POLES: &[&[f32]] = &[CLAP_W, CLAP_GHOST_W];
const BED_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];
// Kick poles (choice - seed for the entire track): strict four
// and syncopations with quiet jolts (ref: Bad Angel). Tremors - whispers
// (0.3), not a second beat: hot eighths produced a jumping beat.
const KICK_SYNCOP_W: &[f32] = &[
    1.0, 0.0, 0.0, 0.3, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.35, 0.0,
];
const KICK_DRIVE8_W: &[f32] = &[
    // Inhale before one: the 15th step is always empty, otherwise the link
    // 14-15-0 gives a flam queue instead of a groove.
    1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.35, 0.0,
];
// Drive with offbeat pushes: four plus quiet syncopation on the weak ones.
const KICK_PUSH_W: &[f32] = &[
    1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.3, 1.0, 0.0, 0.3, 0.0, 1.0, 0.0, 0.0, 0.0,
];
// Shaker: sand 16ths, offbeat eighths - anchors (flat shaker),
// quarters and 16ths are filled in.
const SHAKER_W: &[f32] = &[
    0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.3, 0.5, 0.3, 1.0, 0.4,
];
const KICK_POLES: &[&[f32]] = &[FOUR_FLOOR, KICK_SYNCOP_W, KICK_DRIVE8_W, KICK_PUSH_W];
const HAT_OFF_W: &[f32] = &[
    0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55, 0.0, 0.55, 0.9, 0.55,
];
// Tight sixteenth notes (ref: all three tracks keep the hi-hat for almost the entire bar).
const HAT_BUSY16_W: &[f32] = &[
    0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.4, 0.6, 0.4, 0.8, 0.4, 0.6, 0.4, 0.9, 0.5,
];
const HAT_POLES: &[&[f32]] = &[HAT_W, HAT_OFF_W, HAT_BUSY16_W];

pub struct ClubStyle {}

impl ClubStyle {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ClubStyle {
    fn default() -> Self {
        Self::new()
    }
}

impl Style for ClubStyle {
    fn name(&self) -> &str {
        "club"
    }

    fn bpm(&self) -> f32 {
        126.0
    }

    fn steps_per_measure(&self) -> usize {
        16
    }

    fn swing_ratio(&self) -> f32 {
        // Deep house quality 54–58%: swing 16ths instead of straight ones.
        1.2
    }

    fn style_contract(&self) -> super::contract::StyleContract {
        // Club: the bass gives way to the kick, the phrase breathes with a velocity arch,
        // no more than three melodic onsets per step, upper register 78.
        super::contract::StyleContract::new(true, true, 3, Some(78))
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
        0
    }

    fn density(&self) -> f32 {
        1.0
    }

    /// Club hook: flat eighth notes with skips on weak beats,
    /// the first cell returns after a measure. Downbeats stretch
    /// to the chord - the theme sings with the harmony, not on top of it.
    fn melody(&self) -> Melody {
        Melody {
            onsets: super::phrase_mask(0b1011_1010_1011_0110),
            chord_snap: 0.65,
            sustain: 1.0,
            cells: [0, 1, 0, 3],
            phrased: true,
        }
    }

    /// Style form: your own section lengths - the style should be different
    /// not in timbres, but in construction. seed chooses one of three drawings.
    fn arrangement(&self) -> crate::arranger::Score {
        crate::arranger::HOUSE_SCORE
    }

    fn arrangement_for_seed(&self, seed: u64) -> crate::arranger::Score {
        use crate::arranger::{Score, SongSection};
        const STEADY: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 8),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 12),
                (SongSection::PostChorus, 4),
            ],
        };
        const LONG_GROOVE: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 10),
                (SongSection::BuildUp, 4),
                (SongSection::Drop, 10),
                (SongSection::PostChorus, 4),
            ],
        };
        const SHORT_ARC: Score = Score {
            cycle: &[
                (SongSection::MainMotif, 6),
                (SongSection::BuildUp, 2),
                (SongSection::Drop, 10),
                (SongSection::PostChorus, 4),
            ],
        };
        crate::arranger::pick_score(seed, 0xC1B5_F0A7, &[STEADY, LONG_GROOVE, SHORT_ARC])
    }

    /// Club arrangement: the grid never stalls, effects come
    /// only for the drop, the post-chorus is short (this is a club, not half-time).
    fn arrangement_layers(&self) -> crate::piece::Layers {
        use crate::piece::{default_layers, L_GROOVE, L_HARMONY, L_TEXTURE, L_THEME};
        let base = default_layers();
        [
            // Theme: groove, bass, theme and Rhodes stabs.
            // the entrance to the drop is heard as an event.
            L_GROOVE | L_HARMONY | L_THEME | L_TEXTURE,
            // Build-up: the theme develops.
            base[1],
            // Drop: the entire cast.
            base[2],
            // Post-chorus: groove and Rhodes; the theme rests.
            L_GROOVE | L_HARMONY | L_TEXTURE,
        ]
    }

    fn transition(&self) -> crate::piece::Transition {
        // Disco cut: the groove rolls on, the voice of the next piece enters
        // on top of it - you can’t hear the junction.
        crate::piece::Transition::Rolling { bars: 2 }
    }

    /// Club: snare roll 16th in the last stroke of build-up and short
    /// the tail after the hit is energetic, but not a machine gun for the entire measure.
    fn transition_gesture(&self) -> Option<Spec> {
        Some(Spec::new(
            Kind::SnareRoll,
            &[Slot::Snare],
            &[Slot::Snare],
            3,
            60,
        ))
    }

    fn riff_kind(&self) -> RiffKind {
        RiffKind::Club
    }

    fn reverb_wet(&self) -> f32 {
        0.08
    }

    fn reverb_size(&self) -> f32 {
        0.3
    }

    /// Master highs cutoff, Hz. Dark analog top: shelf above
    /// 16 kHz is cut off (deep house spec). It was 19,000 - excess air numbers.
    fn treble_cutoff(&self) -> f32 {
        16000.0
    }

    fn ensemble(&self) -> &'static [Slot] {
        // Deep house: kick drum, clap, bass, theme pluck, Rhodes stabs and
        // shaker. There is no hat: the ringing cymbal on top killed the character
        // (a “triangle” was included in the mix instead of a beat) - the shaker keeps the rhythm.
        // Pad is a palette color: some seeds take it, others
        // leave the club dry.
        &[
            Slot::Bass,
            Slot::Kick,
            Slot::Snare,
            Slot::Pluck,
            Slot::Keys,
            Slot::Shaker,
            Slot::Pad,
            Slot::Piano,
        ]
    }

    fn signature_slots(&self) -> &'static [Slot] {
        // The theme pluck and Rhodes stabs are the face of club.
        &[Slot::Pluck, Slot::Keys, Slot::Shaker]
    }

    fn slot_guides(&self) -> [RoleGuide; Slot::COUNT] {
        // The bass follows the progression (jitter 1 - adjacent
        // steps, not jumps): bassline instead of drone on one note.
        // Volume holds the hierarchy: the transient of the kick must cover -
        // The sustain of the bass is below the peak of the kick, the beat is driven by the kick, not the bass.
        let bass = RoleGuide {
            energy_response: 2.2,
            gates: &[1.5, 2.0, 3.0],
            tie_repeats: 0.55,
            vel_base: 0.78,
            vel_energy: 0.25,
            vel_jitter: 0.06,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 1,
            },
            octave_up: 0.08,
            top_pitch: 50,
            vel_high_cut: 0.6,
            bass_fifth: true,
            // The sub sits low: cap 43 (G2), upward jumps are rare.
            // Above is no longer the foundation, but the second melody.
            bass_top: 43,
            // Deep, smooth sub instead of strings: the presence chain is pressed down,
            // the foundation is pure sine. Rock holds the string (0.0).
            // Volume with reserve: a sub without presence is quieter than a string one.
            sub_bias: 0.8,
            ..RoleGuide::pitched(BASS_W, 0, -1, 1, &[1.5, 2.0, 3.0], 0.78)
        };
        let kick = RoleGuide {
            // The fill before the drop is moderate: a machine gun instead of a break
            // breaks the beat (combined with downbeat).
            vel_base: 0.95,
            vel_energy: 0.05,
            poles: Some(KICK_POLES),
            ..RoleGuide::kit(FOUR_FLOOR, 36, 0.95)
        };
        let pad = RoleGuide {
            gates: &[6.0, 8.0, 10.0],
            vel_base: 0.5,
            vel_jitter: 0.0,
            ..RoleGuide::pitched(PAD_W, 0, 0, 0, &[6.0, 8.0, 10.0], 0.5)
        };
        let snare = RoleGuide {
            vel_base: 0.95,
            poles: Some(SNARE_POLES),
            ..RoleGuide::kit(CLAP_W, 60, 0.95)
        };
        let hat = RoleGuide {
            energy_response: 1.2,
            long_offbeats: true,
            // Shorter and quieter: the ringing metal at the top gave a “triangle”
            // instead of a beat and filled the entire top of the mix. The rhythm is kept by the shaker.
            gates: &[0.35, 0.7, 1.1],
            vel_base: 0.42,
            vel_energy: 0.3,
            vel_jitter: 0.1,
            poles: Some(HAT_POLES),
            // Hat holds the grid of poles: his own pulse gave phasing
            // against 16s and disrupted the rhythm - the beat is more important than the drift.
            // The hat has its own groove for each section (the drop has its own beat),
            // the choice is determined by the seed and section.
            section_poles: true,
            ..RoleGuide::kit(HAT_W, 42, 0.65)
        };
        // The beatmaker's recurring theme: the pluck carries the seed motif across sections —
        // the beat has a melodic face, not just a groove. Long notes:
        // the line flows in phrases, rather than poking at one-time hits.
        let pluck = RoleGuide {
            energy_response: 1.5,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.55,
            vel_energy: 0.3,
            vel_jitter: 0.07,
            // Unison theme: carriers sing the same steps (jitter 0) -
            // a solid melody, colors distinguish timbres, not pitches.
            // An octave one lower: the synthesizer at the very top was arguing
            // with an EP and a hat, the line needs to be kept in the middle.
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 0,
            },
            follow_motif: true,
            phrase_rest: 0.2,
            ..RoleGuide::pitched(LEAD_W, 0, 1, 0, &[1.0, 1.5, 2.0], 0.45)
        };
        let keys = RoleGuide {
            // A fluid guest, not a switch: an even presence
            // through sections instead of “either no, then a wall.”
            energy_response: 1.2,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.62,
            vel_jitter: 0.08,
            voice: Voicing::Scale {
                degree: 2,
                octave: 1,
                jitter: 0,
            },
            ..RoleGuide::pitched(STAB_W, 2, 1, 0, &[1.0, 1.5, 2.0], 0.55)
        };
        // 808 keeps the four-floor grid rooted in progression (the same
        // harmonic path that the sub has); slide between notes is the deal
        // Kick808 voices, the generator only puts the roots on the beats.
        // Before the drop there is a sub-roll: a hit instead of a cymbal ringing.
        // The register is the same as that of the sub (fold in mod.rs): 808 - bass, not lead.
        let kick808 = RoleGuide {
            vel_base: 0.95,
            vel_energy: 0.05,
            voice: Voicing::Scale {
                degree: 0,
                octave: -1,
                jitter: 0,
            },
            bass_top: 43,
            ..RoleGuide::pitched(FOUR_FLOOR, 0, -1, 0, &[1.0], 0.95)
        };
        // Alien slots: working but discreet guides - included
        // Only handle the front, the character of the style is not broken.
        let vinyl = RoleGuide {
            gates: &[200.0],
            vel_base: 0.4,
            vel_jitter: 0.05,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[200.0], 0.4)
        };
        // Alien melodic/textures: sparse guides.
        // The guitar in the club is a manual guest (rhythm guitar instead of pluck).
        let guitar = RoleGuide {
            energy_response: 1.2,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.4,
            voice: Voicing::Scale {
                degree: 0,
                octave: 1,
                jitter: 3,
            },
            ..RoleGuide::pitched(LEAD_W, 0, 1, 3, &[1.0, 1.5, 2.0], 0.4)
        };
        // Deep house shaker: 16's sand on top of the hi-hat, emphasis on offbeats.
        // Rain: an even veil in all phrases (quieter than vinyl and music).
        let rain = RoleGuide {
            gates: &[150.0],
            vel_base: 0.35,
            ..RoleGuide::pitched(BED_W, 0, 0, 0, &[150.0], 0.35)
        };
        let shaker = RoleGuide {
            energy_response: 1.0,
            gates: &[0.4],
            vel_base: 0.62,
            vel_energy: 0.15,
            vel_jitter: 0.08,
            ..RoleGuide::kit(SHAKER_W, 60, 0.62)
        };
        // Lead guitar outside club automix: manual color for the frontend.
        let guitar_lead = RoleGuide {
            energy_response: 1.0,
            gates: &[1.0, 1.5, 2.0],
            vel_base: 0.35,
            voice: Voicing::Scale {
                degree: 0,
                octave: 2,
                jitter: 1,
            },
            ..RoleGuide::pitched(BED_W, 0, 2, 1, &[1.0, 1.5, 2.0], 0.35)
        };
        // The piano is the quiet color of the club: the same stabs, but acoustic
        // hammer attack tone instead of Rhodes.
        let piano = RoleGuide {
            energy_response: 0.8,
            gates: &[2.0, 3.0, 4.0],
            vel_base: 0.4,
            ..RoleGuide::pitched(PAD_W, 0, 0, 1, &[2.0, 3.0, 4.0], 0.4)
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

#[cfg(test)]
mod tests {
    use super::super::{render_style, StepContext};
    use super::*;
    use crate::playback::{HAT_ID, KICK808_ID, KICK_ID, SNARE_ID};

    fn render(seed: u64, steps: usize) -> Vec<(usize, crate::instruments::NoteEvent)> {
        // Testing the guides, not the mix: all slots are active.
        let style = ClubStyle::new();
        let active: Vec<usize> = crate::instruments::Slot::ALL
            .iter()
            .map(|s| s.id())
            .collect();
        let mut rng = rand::SeedableRng::seed_from_u64(seed);
        let mut arranger = crate::arranger::Arranger::new(seed);
        let mut song = arranger.advance_measure();
        let mut out = Vec::new();
        for step in 0..steps {
            let spm = style.steps_per_measure();
            let sim = step % spm;
            if sim == 0 && step > 0 {
                song = arranger.advance_measure();
            }
            let mut sctx = StepContext {
                step,
                step_in_measure: sim,
                base_step_secs: style.base_step_secs(),
                active_tracks: &active,
                song: &song,
                rng: &mut rng,
                intensity: 1.0,
            };
            out.extend(
                render_style(&style, &mut sctx)
                    .into_iter()
                    .map(|n| (step, n)),
            );
        }
        out
    }

    #[test]
    fn downbeat_breathes_but_holds() {
        // Anchor 1: skipping is possible (priority 0.95), but downbeat holds
        // in the overwhelming majority of bars - the face of the genre is intact.
        let notes = render(11, 1024);
        let downs = 1024 / 16;
        let hits = notes
            .iter()
            .filter(|(step, n)| n.instrument_id == KICK_ID && step % 16 == 0)
            .count();
        assert!(hits * 10 >= downs * 9, "downbeat lost: {hits}/{downs}");
    }

    #[test]
    fn same_seed_same_notes() {
        let a = render(1234, 32);
        let b = render(1234, 32);
        assert_eq!(a.len(), b.len());
        for ((_, x), (_, y)) in a.iter().zip(b.iter()) {
            assert_eq!(x.instrument_id, y.instrument_id);
            assert_eq!(x.midi_pitch, y.midi_pitch);
        }
    }

    #[test]
    fn kick808_is_four_on_floor() {
        // After entering the intro (steps 32+): four-floor holds.
        let notes = render(7, 128);
        let quarters_with_kick = notes
            .iter()
            .filter(|(step, n)| n.instrument_id == KICK808_ID && *step >= 32 && step % 4 == 0)
            .count();
        assert!(
            quarters_with_kick >= 20,
            "four-on-the-floor broken: {quarters_with_kick}"
        );
    }

    #[test]
    fn all_pitches_are_kit_or_scale() {
        for (_, n) in render(99, 64) {
            // 808 melodic (follows the bassline), the rest
            // drums - fixed set. The clap is marked with a height of 60:
            // The snare plays on it with a clap, not a drum.
            if [KICK_ID, SNARE_ID, HAT_ID].contains(&n.instrument_id) {
                assert!([36, 38, 42, 60].contains(&n.midi_pitch));
            } else {
                assert!((28..=90).contains(&n.midi_pitch));
            }
        }
    }
}
