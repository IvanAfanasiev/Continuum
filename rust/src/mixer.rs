use std::collections::HashMap;

use crate::core::MAX_ACTIVE_TRACKS;
use crate::instruments::{Group, Slot};

/// Mixer state: two volume layers.
///
/// - Slots (`tracks`): style defaults + per-slot edits. 0 = muted.
/// - Groups (`groups`): frontend macro faders (multiplier 0..1,
///   default 1.0). One "Kick" instead of "punchy vs 808".
///
/// Effective volume = slot × group. Zero on either layer means
/// "not in use": the composer generates no notes, the engine spends no DSP.
/// A style change resets SLOTS to the new style's and seed's cast, but NOT
/// groups: macro faders ("more kick", "quieter leads") are the
/// listener's setting and survive transitions between styles.
/// If the listener edited the instrument list themselves (`list_edited`), a change
/// of style and seed doesn't touch it: manual choice takes priority. Reset is the
/// "reset settings" button in the mixer.
#[derive(Clone, Default)]
pub struct MixerState {
    pub tracks: HashMap<usize, f32>,
    pub groups: HashMap<Group, f32>,
    /// The listener changed the instrument list: style defaults don't
    /// overwrite it until reset is pressed.
    pub list_edited: bool,
}

impl MixerState {
    pub fn new() -> Self {
        Self {
            tracks: HashMap::new(),
            groups: HashMap::new(),
            list_edited: false,
        }
    }

    /// Slot volume 0..1. Zero = slot excluded.
    pub fn set_track(&mut self, id: usize, volume: f32) {
        self.tracks.insert(id, volume.clamp(0.0, 1.0));
    }

    /// Listener's slot edit: the list is marked as edited, and further changes
    /// of style/seed no longer overwrite it (manual choice takes priority).
    pub fn set_track_by_user(&mut self, id: usize, volume: f32) {
        self.set_track(id, volume);
        self.list_edited = true;
    }

    /// Group macro fader 0..1 (can't go above the default —
    ///   the frontend scale is 0..100% everywhere).
    pub fn set_group(&mut self, group: Group, volume: f32) {
        self.groups.insert(group, volume.clamp(0.0, 1.0));
    }

    /// Reset groups to default (1.0). Normally NOT called on a style
    /// change (the listener's faders survive transitions); this is an emergency
    /// valve for the single case where inherited faders would
    /// mute the new style entirely.
    pub fn reset_groups(&mut self) {
        self.groups.clear();
    }

    pub fn group_volume(&self, group: Group) -> f32 {
        self.groups.get(&group).copied().unwrap_or(1.0)
    }

    /// Effective volume: product of the layers, 0 for missing entries.
    /// Single source of truth for the engine and composer.
    pub fn volume_for(&self, id: usize) -> f32 {
        let slot_vol = self.tracks.get(&id).copied().unwrap_or(0.0);
        if slot_vol <= 0.0 {
            return 0.0;
        }
        let group_vol = Slot::ALL
            .iter()
            .find(|s| s.id() == id)
            .map(|s| self.group_volume(s.group()))
            .unwrap_or(1.0);
        slot_vol * group_vol
    }

    /// Is there at least one audible slot? Last-instrument guard:
    /// muting that would leave nothing is rejected.
    pub fn has_audible(&self) -> bool {
        self.tracks.keys().any(|id| self.volume_for(*id) > 0.0)
    }

    /// Fills `out` with sorted ids of used tracks
    /// (effective volume > 0), returns their count.
    pub fn active_ids(&self, out: &mut [usize; MAX_ACTIVE_TRACKS]) -> usize {
        let mut n = 0;
        for id in self.tracks.keys() {
            if self.volume_for(*id) > 0.0 && n < MAX_ACTIVE_TRACKS {
                out[n] = *id;
                n += 1;
            }
        }
        out[..n].sort_unstable();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_clamped() {
        let mut m = MixerState::new();
        m.set_track(1, 5.0);
        assert_eq!(m.volume_for(1), 1.0);
        m.set_track(1, -2.0);
        assert_eq!(m.volume_for(1), 0.0);
    }

    #[test]
    fn zero_volume_means_excluded() {
        let mut m = MixerState::new();
        m.set_track(1, 0.0);
        assert_eq!(m.volume_for(1), 0.0);
        assert_eq!(m.volume_for(999), 0.0);
        let mut buf = [0usize; MAX_ACTIVE_TRACKS];
        assert_eq!(m.active_ids(&mut buf), 0);
    }

    #[test]
    fn last_audible_is_protected() {
        let mut m = MixerState::new();
        m.set_track(1, 0.8);
        m.set_track(2, 0.5);
        assert!(m.has_audible());
        // Mute everything but one — still audible.
        let mut probe = m.clone();
        probe.set_track(1, 0.0);
        assert!(probe.has_audible());
        // Mute the last one — silence; we reject such a change.
        probe.set_track(2, 0.0);
        assert!(!probe.has_audible());
        // A group at zero with live slots — also silence.
        let mut probe2 = m.clone();
        probe2.set_group(Group::Bass, 0.0);
        probe2.set_group(Group::Kick, 0.0);
        assert!(!probe2.has_audible());
    }

    #[test]
    fn group_fader_multiplies_slots() {
        let mut m = MixerState::new();
        m.set_track(2, 0.8); // punchy kick
        m.set_track(8, 0.8); // 808 kick — same Kick group
        assert_eq!(m.volume_for(2), 0.8);
        m.set_group(Group::Kick, 0.5);
        assert_eq!(m.volume_for(2), 0.4);
        assert_eq!(m.volume_for(8), 0.4);
    }

    #[test]
    fn group_zero_excludes_whole_group() {
        let mut m = MixerState::new();
        m.set_track(2, 0.8);
        m.set_track(8, 0.8);
        m.set_group(Group::Kick, 0.0);
        let mut buf = [0usize; MAX_ACTIVE_TRACKS];
        assert_eq!(m.active_ids(&mut buf), 0);
    }

    #[test]
    fn active_ids_sorted_and_bounded() {
        let mut m = MixerState::new();
        m.set_track(3, 0.5);
        m.set_track(1, 0.5);
        m.set_track(2, 0.0);
        let mut buf = [0usize; MAX_ACTIVE_TRACKS];
        let n = m.active_ids(&mut buf);
        assert_eq!(&buf[..n], &[1, 3]);
    }
}
