//! Каст сида: состав инструментов из палитры стиля.
//!
//! Палитра (`Style::ensemble`) — это краски стиля, а не жёсткий состав:
//! сид выбирает подмножество, поэтому две композиции одного стиля могут
//! звучать камерно, а могут — плотно, не теряя узнаваемости.
//!
//! Правила простые и музыкальные:
//! - фундамент (бас, кик) и подписные инструменты стиля остаются всегда;
//! - снейр держит бит, а хэт/шейкер — пульс: хотя бы один из них звучит;
//! - гармония и тема не пропадают: если в палитре есть аккорды/лиды,
//!   хотя бы один представитель группы выбран;
//! - вокал, текстуры, второй лид и 808 — краски: сид решает, нужны ли они;
//! - слоты стилевого перехода входят в каст всегда, иначе жест на шве
//!   остался бы без инструмента.
//!
//! Краски выбираются не независимыми бросками, а общим рейтингом:
//! сид задаёт плотность каста (35–85%), а слоты занимают места по
//! собственному рангу. Поэтому редкий сид не «теряет» все краски сразу
//! и не берёт их все скопом, а состав меняется от сида к сиду.
//!
//! Решение детерминировано (сид, стиль, слот): тот же сид — тот же состав.

use crate::arranger::splitmix64;
use crate::instruments::{Group, Slot};

const CAST_SALT: u64 = 0xCA57_0D1E_5EED;

/// Соль стиля: одинаковый сид в разных стилях даёт разные касты.
fn style_salt(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Если в группе нет ни одного выбранного слота, выбрать один из палитры.
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

/// Собрать каст из палитры стиля.
///
/// `signature` — инструменты, без которых стиль теряет лицо (риффовая
/// гитара рока, Rhodes лофай и т.п.); `required` — слоты перехода.
pub fn from_palette(
    palette: &[Slot],
    seed: u64,
    style_name: &str,
    signature: &[Slot],
    required: &[Slot],
) -> Vec<Slot> {
    let hash = splitmix64(seed ^ CAST_SALT ^ style_salt(style_name));
    let mut chosen: Vec<Slot> = Vec::with_capacity(palette.len());

    // Фундамент, подписи и переходы не обсуждаются: без них стиль или
    // шов теряет смысл. Остальное — краски, их решает сид.
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

    // Плотность каста: 35–85% красок. Узкий сид берёт только главное,
    // плотный — почти всё, но не выходит за палитру стиля.
    let density = 35 + splitmix64(hash ^ 0xD3A5_11CE) % 51;
    let take = ((colors.len() as u64 * density + 50) / 100) as usize;
    // Ранг слота — его место в очереди на этот сид. Порядок палитры
    // остаётся стабильным, меняется только состав.
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

    // Музыка не должна проваливаться в дыру: пульс, гармония и тема
    // живут в каждом касте.
    ensure_group(&mut chosen, palette, Group::Kick, hash);
    ensure_group(&mut chosen, palette, Group::Bass, hash);
    ensure_group(&mut chosen, palette, Group::Hat, hash);
    ensure_group(&mut chosen, palette, Group::Chords, hash);
    ensure_group(&mut chosen, palette, Group::Lead, hash);

    // Порядок палитры: стабильный обход движка.
    chosen.sort_by_key(|s| s.index());
    chosen.dedup();
    chosen
}
