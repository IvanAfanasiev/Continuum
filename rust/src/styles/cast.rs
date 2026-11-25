//! Seed cast: composition of instruments from the style palette.
//!
//! The palette (`Style::ensemble`) is the colors of the style, not a rigid composition:
//! seed selects a subset, so two compositions of the same style can sound
//! chamber-like or dense, without losing their identity.
//!
//! The rules are simple and musical:
//! - the foundation (bass, kick) and signature instruments of the style always remain;
//! - the snare holds the beat, and the hat/shaker holds the pulse: at least one of them sounds;
//! - harmony and theme do not disappear: if there are chords/leads in the palette,
//!   at least one group representative has been selected;
//! - textures, second lead and 808 are colors: the seed decides whether they are needed;
//! - style transition slots are always included in the cast; otherwise the seam gesture
//!   would be left without an instrument.
//!
//! Colors are chosen not by independent rolls, but by an overall ranking:
//! The seed sets cast density (35–85%), and slots occupy places according to their
//! own rank. A sparse seed therefore does not “lose” all its colors at once
//! and does not take them all en masse, but the composition changes from seed to seed.
//!
//! The decision is deterministic (seed, style, slot): the same seed — the same cast.

use crate::arranger::splitmix64;
use crate::instruments::{Group, Slot};

const CAST_SALT: u64 = 0xCA57_0D1E_5EED;

/// Style salt: the same seed in different styles gives different casts.
fn style_salt(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// If there is no selected slot in the group, select one from the palette.
fn ensure_group(chosen: &mut Vec<Slot>, palette: &[Slot], group: Group, hash: u64) {
    if chosen.iter().any(|s| s.group() == group) {
        return;
    }
    let candidates: Vec<Slot> = palette
        .iter()
        .copied()
        .filter(|s| s.group() == group)
        .collect();
    if candidates.is_empty() {
        return;
    }
    let idx = (splitmix64(hash ^ group.index() as u64) % candidates.len() as u64) as usize;
    chosen.push(candidates[idx]);
}

/// Build a cast from the style palette.
///
/// `signature` — instruments without which the style loses its identity
/// (rock riff guitar, Rhodes in lofi, etc.); `required` — transition slots.
pub fn from_palette(
    palette: &[Slot],
    seed: u64,
    style_name: &str,
    signature: &[Slot],
    required: &[Slot],
) -> Vec<Slot> {
    let hash = splitmix64(seed ^ CAST_SALT ^ style_salt(style_name));
    let mut chosen: Vec<Slot> = Vec::with_capacity(palette.len());

    // The foundation, signatures and transitions are non-negotiable: without them the style
    // or the seam loses its meaning. The rest is color, decided by the seed.
    let mut colors: Vec<Slot> = Vec::new();
    for slot in palette {
        let essential = matches!(slot.group(), Group::Bass | Group::Snare)
            || *slot == Slot::Kick
            || signature.contains(slot)
            || required.contains(slot);
        if essential {
            chosen.push(*slot);
        } else {
            colors.push(*slot);
        }
    }

    // Cast density: 35–85% of the colors. A sparse seed takes only the essentials,
    // a dense one takes almost everything, but never leaves the style palette.
    let density = 35 + splitmix64(hash ^ 0xD3A5_11CE) % 51;
    let take = ((colors.len() as u64 * density + 50) / 100) as usize;
    // The rank of a slot is its place in the queue for this seed. Palette order
    // remains stable, only the composition changes.
    let mut ranked: Vec<(u64, Slot)> = colors
        .iter()
        .map(|slot| {
            let rank =
                splitmix64(hash ^ (slot.index() as u64).wrapping_mul(0x9E37_79B9) ^ 0xC0FF_EE00);
            (rank, *slot)
        })
        .collect();
    ranked.sort_by_key(|(rank, slot)| (*rank, slot.index()));
    chosen.extend(ranked.iter().take(take).map(|(_, slot)| *slot));

    // Music must not fall into a hole: pulse, harmony and theme
    // live in every cast.
    ensure_group(&mut chosen, palette, Group::Kick, hash);
    ensure_group(&mut chosen, palette, Group::Bass, hash);
    ensure_group(&mut chosen, palette, Group::Hat, hash);
    ensure_group(&mut chosen, palette, Group::Chords, hash);
    ensure_group(&mut chosen, palette, Group::Lead, hash);

    // Palette order: stable engine traversal.
    chosen.sort_by_key(|s| s.index());
    chosen.dedup();
    chosen
}
