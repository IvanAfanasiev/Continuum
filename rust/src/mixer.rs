use std::collections::HashMap;

use crate::core::MAX_ACTIVE_TRACKS;
use crate::instruments::{Group, Slot};

/// Состояние микшера: два слоя громкости.
///
/// - Слоты (`tracks`): дефолты стиля + точечные правки. 0 = выключен.
/// - Группы (`groups`): макро-фейдеры фронта (множитель 0..1,
///   дефолт 1.0). Один «Kick» вместо «punchy vs 808».
///
/// Эффективная громкость = слот × группа. Ноль на любом слое означает
/// «не используется»: композер не генерирует ноты, движок не тратит DSP.
/// Смена стиля сбрасывает СЛОТЫ на дефолт нового стиля (вайтлист + гости
/// сида), но НЕ группы: макро-фейдеры («больше кика», «тише лиды») —
/// настройка слушателя, она переживает переход между стилями.
/// Если слушатель правил список инструментов сам (`list_edited`), смена
/// стиля и сида его не трогает: ручной выбор — приоритет. Сброс — кнопка
/// «сброс настроек» в миксе.
#[derive(Clone, Default)]
pub struct MixerState {
    pub tracks: HashMap<usize, f32>,
    pub groups: HashMap<Group, f32>,
    /// Список инструментов менял слушатель: дефолты стиля его не
    /// перетирают, пока не нажат сброс.
    pub list_edited: bool,
    /// Назначения «инструмент играет партию»: слот → слот-источник.
    /// Ключ — id играющего слота, значение — id слота, чью партию он
    /// исполняет. Нет записи = слот играет свою партию.
    ///
    /// Назначение общее для стилей: в rock пианино может играть рифф,
    /// в club — club-овскую линию. Смена стиля назначение не сбрасывает.
    pub lines: HashMap<usize, usize>,
}

impl MixerState {
    pub fn new() -> Self {
        Self {
            tracks: HashMap::new(),
            groups: HashMap::new(),
            list_edited: false,
            lines: HashMap::new(),
        }
    }

    /// Громкость слота 0..1. Ноль = слот исключен.
    pub fn set_track(&mut self, id: usize, volume: f32) {
        self.tracks.insert(id, volume.clamp(0.0, 1.0));
    }

    /// Правка слота слушателем: список помечается изменённым, и смена
    /// стиля/сида его больше не перетирает (ручной выбор — приоритет).
    pub fn set_track_by_user(&mut self, id: usize, volume: f32) {
        self.set_track(id, volume);
        self.list_edited = true;
    }

    /// Назначить слоту чужую партию (id источника). `None` — слот
    /// играет свою партию.
    pub fn set_line(&mut self, id: usize, line: Option<usize>) {
        match line {
            Some(src) if src != id => {
                self.lines.insert(id, src);
            }
            _ => {
                self.lines.remove(&id);
            }
        }
    }

    /// Чью партию играет слот: свою или назначенную.
    pub fn line_for(&self, id: usize) -> usize {
        self.lines.get(&id).copied().unwrap_or(id)
    }

    /// Макро-фейдер группы 0..1 (выше дефолта не поднять —
    /// шкала фронта везде 0..100%).
    pub fn set_group(&mut self, group: Group, volume: f32) {
        self.groups.insert(group, volume.clamp(0.0, 1.0));
    }

    /// Сброс групп на дефолт (1.0). Штатно НЕ вызывается при смене
    /// стиля (фейдеры слушателя переживают переход); это аварийный
    /// клапан для единственного случая, когда унаследованные фейдеры
    /// заглушили бы новый стиль целиком.
    pub fn reset_groups(&mut self) {
        self.groups.clear();
    }

    pub fn group_volume(&self, group: Group) -> f32 {
        self.groups.get(&group).copied().unwrap_or(1.0)
    }

    /// Эффективная громкость: произведение слоев, 0 для отсутствующих.
    /// Единая точка истины для движка и композера.
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

    /// Есть ли хоть один слышимый слот. Защита последнего инструмента:
    /// глушение, после которого не останется ничего, отклоняется.
    pub fn has_audible(&self) -> bool {
        self.tracks.keys().any(|id| self.volume_for(*id) > 0.0)
    }

    /// Заполняет `out` отсортированными id используемых треков
    /// (эффективная громкость > 0), возвращает их число.
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
        // Глушим все, кроме одного, — еще слышно.
        let mut probe = m.clone();
        probe.set_track(1, 0.0);
        assert!(probe.has_audible());
        // Глушим последний — тишина, такое изменение отклоняем.
        probe.set_track(2, 0.0);
        assert!(!probe.has_audible());
        // Группа в ноль при живых слотах — тоже тишина.
        let mut probe2 = m.clone();
        probe2.set_group(Group::Bass, 0.0);
        probe2.set_group(Group::Kick, 0.0);
        assert!(!probe2.has_audible());
    }

    #[test]
    fn group_fader_multiplies_slots() {
        let mut m = MixerState::new();
        m.set_track(2, 0.8); // punchy kick
        m.set_track(8, 0.8); // 808 kick — та же группа Kick
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
