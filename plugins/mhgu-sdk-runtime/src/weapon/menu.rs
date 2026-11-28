//! GU equipment-box inventory, status and equipment-set panels.
use crate::weapon::records;
use super::{gu, icon, inventory, local_hunter, profile, read, set, sets};
use core::ffi::CStr;
use mhgu_sdk::sync::Shared;

const ACCEPT: u32 = 0x2000;
const BACK: u32 = 0x4000;

/// GU equipment description layout (57FF24 and 58565C), populated from
/// an SDK definition instead of indexing the native equipment catalogue.
pub(super) unsafe fn details(
    panel: u32,
    entry: Option<inventory::RegisteredEquipment>,
    level: u32,
    jewels: crate::weapon::decorations::Jewels,
) {
    if panel != 0 {
        unsafe {
            gu!(0x57f468, panel, 0, 0, 0, 0, 0, 0, 0);
            set(panel, 80, 0u32);
            details_with_stats(panel, entry, level, jewels, DetailStats::default());
        }
    }
}

#[derive(Clone, Copy, Default)]
struct DetailStats {
    equipment: u32,
    compared_record: u32,
    compared_equipment: u32,
}

unsafe fn details_with_stats(
    panel: u32,
    entry: Option<inventory::RegisteredEquipment>,
    level: u32,
    jewels: crate::weapon::decorations::Jewels,
    stats: DetailStats,
) {
    if panel == 0 {
        return;
    }
    unsafe {
        if let Some(entry) = entry {
            gu!(0x56e528, panel, 17, 1);

            gu!(0x56ea08, panel, 17, 3, entry.name.as_ptr() as u32);
            icon::equipment(panel, 17, 2, entry);
            let messages = read::<u32>(forge::mem::text_addr(), 0x1899220);
            if messages != 0 {
                gu!(0x56ea08, panel, 17, 4, gu!(0x55bc4c, messages, 1, 30));
            }
            gu!(0x56fe88, panel, 17, 7, level, 0, 0);
            if let Some(upgrade) = level
                .checked_sub(1)
                .filter(|&i| (i as usize) < entry.levels.len())
            {
                level_badge(panel, entry.equipment.id.get(), upgrade as u16);
                if read::<u32>(panel, 248) == 5 {
                    gu!(0x56e528, panel, 6, 1);
                    gu!(0x56ea08, panel, 6, 1, entry.details.as_ptr() as u32);
                    gu!(0x56e530, panel, 6, 5, 0);
                } else {
                    stat_page(
                        panel,
                        records::DisplayEquipment {
                            definition: entry,
                            upgrade: upgrade as u16,
                            jewels,
                        },
                        stats,
                    );
                }
            }
        }
    }
}
// 58565C and 142DE4 use the same three stages for the header and owned count.
fn crown(equipment: u16, upgrade: u16) -> (u32, u32) {
    let mut record = [0u32; 11];
    let Some(header) = super::trade::native_level_header(equipment, upgrade) else { return (0, 0) };
    record[1] = header;
    unsafe {
        let catalog = read::<u32>(forge::mem::text_addr(), 0x1856b18);
        if catalog == 0 { return (0, 0); }
        let row = gu!(0x13f03c, catalog, record.as_ptr() as u32);
        if row == 0 { return (0, 0); }
        let vtable = read::<u32>(row, 0);
        let show: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(read::<u32>(vtable, 128));
        if show(row, (header >> 5) & 31) != 1 { return (0, 0); }
        (1 + gu!(0xdbd74, record.as_ptr() as u32), header)
    }
}

unsafe fn level_badge(panel: u32, equipment: u16, upgrade: u16) {
    if unsafe { read::<u32>(panel, 36) } == 0 { return; }
    let (stage, header) = crown(equipment, upgrade);
    let mut record = [0u32; 11];
    record[1] = header;
    unsafe {
        gu!(0x56edb0, panel, 17, 1, stage.saturating_sub(1), u32::from(stage != 0));
        let color = gu!(0x550534, if header == 0 { 0 } else { record.as_ptr() as u32 });
        gu!(0x56e6fc, panel, 17, 7, color);
    }
}

pub(super) fn owned_count(equipment: u16, upgrade: u16) -> (u32, bool) {
    let (counts, room) = crate::save::CHARACTER.with(|save| {
        let mut counts = alloc::collections::BTreeMap::<u16, u32>::new();
        for item in save.inventory.instances.values().filter(|i| i.equipment == equipment) {
            *counts.entry(item.upgrade).or_default() += 1;
        }
        (counts, save.inventory.instances.len() < crate::save::INVENTORY_LIMIT)
    });
    let stage = crown(equipment, upgrade).0;
    let count = counts.into_iter().filter(|(level, _)| crown(equipment, *level).0 == stage)
        .map(|(_, quantity)| quantity).sum();
    (count, room)
}

fn affinity(base: i32, equipment: u32) -> (i32, bool) {
    unsafe {
        // DEE34 stores base affinity at cEquip+338. Only use GU's skill
        // result when the aggregate was calculated from the displayed base.
        if equipment != 0
            && i32::from(read::<i16>(equipment, 338)) == base
            && gu!(0xe1bb0, equipment + 4, 16) != 0
        {
            return ((gu!(0xe173c, equipment + 4, 16) as i32).min(100), true);
        }
    }
    (base, false)
}

unsafe fn stat_color(panel: u32, group: u32, part: u32, color: u32) {
    unsafe {
        let palette = read::<u32>(forge::mem::text_addr(), 0x1899458);
        gu!(0x56e6fc, panel, group, part, gu!(0x560e90, palette, color));
    }
}

fn comparison_color(value: i32, compared: Option<i32>, default: u32) -> u32 {
    match compared.map(|other| value.cmp(&other)) {
        Some(core::cmp::Ordering::Less) => 3,
        Some(core::cmp::Ordering::Greater) => 10,
        _ => default,
    }
}

/// Populate GU's weapon stat groups from the displayed instance on either
/// side of the comparison. The scratch record supplies identity to native widgets.
unsafe fn stat_page(panel: u32, weapon: records::DisplayEquipment, stats: DetailStats) {
    unsafe {
        let e = weapon.definition;
        let level = weapon.level();
        let jewels = weapon.jewels;
        let scratch = [0u32; 11];
        let record = scratch.as_ptr() as u32;
        let compared = stats.compared_record;
        let flags = (read::<u32>(panel, 80) & !0xc000)
            | (u32::from(compared != 0) * 0x4000)
            | (u32::from(stats.equipment != 0) * 0x8000);
        set(panel, 80, flags);
        set(panel, 184, stats.equipment);
        set(panel, 188, stats.compared_equipment);
        let ranged = e.kind.ranged;
        let group = if ranged { 4 } else { 2 };
        let layout = read::<u32>(panel, 36);
        if layout != 0 {
            gu!(0x54533c, layout, if ranged { 2 } else { 4 }, 0);
        }
        gu!(0x56e528, panel, group, 1);
        let messages = read::<u32>(forge::mem::text_addr(), 0x1899220);
        if messages == 0 {
            return;
        }
        let hidden: &[u32] = if ranged {
            &[20, 28]
        } else {
            &[3, 4, 5, 6, 7, 33, 34, 35, 38, 39, 41, 42, 43, 44, 45, 50]
        };
        for &part in hidden {
            gu!(0x56e530, panel, group, part, 0);
        }
        let compared_level = records::for_record(compared).map(|other| other.level());
        let other_attack = (compared != 0).then(|| {
            compared_level.map_or_else(|| gu!(0xdb7e0, compared) as i32, |l| i32::from(l.attack))
        });
        let other_affinity = (compared != 0).then(|| {
            let base = compared_level
                .map_or_else(|| gu!(0xdb88c, compared) as i32, |l| i32::from(l.affinity));
            affinity(base, stats.compared_equipment).0
        });
        let attack = i32::from(level.attack);
        gu!(0x56e808, panel, group, 1, gu!(0x55bc4c, messages, 1, 54));
        gu!(0x56fde0, panel, group, 2, attack as u32, 0, 0);
        stat_color(panel, group, 2, comparison_color(attack, other_attack, 0));

        let (affinity_value, enhanced) = affinity(i32::from(level.affinity), stats.equipment);
        let (label, penalty, number, penalty_label, percent) =
            if ranged { (7, 8, 10, 9, 11) } else { (25, 26, 28, 27, 29) };
        gu!(0x56e530, panel, group, penalty, 0);
        gu!(0x56e530, panel, group, penalty_label, 0);
        gu!(0x56e808, panel, group, label, gu!(0x55bc4c, messages, 9, 261));
        gu!(0x56fde0, panel, group, number, affinity_value as u32, 0, 0);
        stat_color(panel, group, label, if enhanced { 6 } else { 5 });
        let color = comparison_color(affinity_value, other_affinity, u32::from(enhanced) * 6);
        stat_color(panel, group, number, color);
        stat_color(panel, group, percent, color);

        records::with_record(record, weapon, || {
            if ranged {
                gu!(0x586398, panel, record, compared, 4, 12, 14, 13);
            } else {
                gu!(0x586398, panel, record, compared, 2, 30, 32, 31);
                if e.equipment.elements.iter().any(|element| element.id() != 0) {
                    let first = [2u32, 38 | (5 << 16), 6];
                    let second = [2u32, 42 | (43 << 16), 44];
                    gu!(
                        0x586574,
                        panel,
                        record,
                        compared,
                        first.as_ptr() as u32,
                        second.as_ptr() as u32
                    );
                }
            }
        });
        let slot_label = if ranged { 3 } else { 21 };
        gu!(
            0x56e808,
            panel,
            group,
            slot_label,
            gu!(0x55bc4c, messages, 9, 260)
        );
        crate::weapon::decorations::draw(panel, group, slot_label + 1, level.slots, jewels);
        if ranged {
            return;
        }
        sharpness_bar(panel, level, stats.equipment, stats.compared_equipment);
    }
}

fn handicraft(equipment: u32) -> u32 {
    if equipment == 0 {
        return 0;
    }
    let has_skill = |skill| unsafe { gu!(0x26fe08, equipment, skill) != 0 };
    if [83, 277, 295].into_iter().any(has_skill) {
        2
    } else {
        u32::from([84, 281, 299, 309].into_iter().any(has_skill))
    }
}

unsafe fn sharpness_bar(panel: u32, level: mhgu_sdk::weapon::EquipmentLevel, aggregate: u32, compared: u32) {
    unsafe {
        let messages = read::<u32>(forge::mem::text_addr(), 0x1899220);
        if messages == 0 { return; }
        let layout = read::<u32>(panel, 36);
        let Some(curve) = super::stats::sharpness_curve(level.sharpness_type) else {
            return;
        };
        if layout == 0 {
            return;
        }
        gu!(0x56e808, panel, 2, 8, gu!(0x55bc4c, messages, 9, 259));
        let base_rank = u32::from(level.sharpness_level);
        let bonus = if base_rank < 12 { handicraft(aggregate) } else { 0 };
        let limit = 150 + 25 * (base_rank + bonus).min(12);
        let scale = read::<f32>(panel, 192) / 450.;
        let width_at = |boundary| (scale * u32::from(boundary).min(limit) as f32) as u32 as f32;
        let set_part = |offset: u32, part: u32, value: f32| {
            core::arch::asm!("blx {draw}", draw = in(reg) forge::mem::text_addr() + offset,
                in("r0") layout, in("r1") 2u32, in("r2") part, in("s0") value, clobber_abi("C"));
        };
        for (i, boundary) in curve.into_iter().enumerate() {
            let part = read::<u32>(forge::mem::text_addr(), 0x163010c + 4 * i as u32);
            gu!(0x56e530, panel, 2, part, 1);
            set_part(0x546afc, part, width_at(boundary));
        }
        gu!(0x56e530, panel, 2, 9, u32::from(bonus == 2));
        gu!(0x56e530, panel, 2, 10, u32::from(bonus == 1));
        gu!(0x56e530, panel, 2, 20, u32::from(bonus != 0));
        if bonus != 0 {
            // 5859F4 anchors the selected marker at the end of the coloured bar.
            let start = gu!(0x545ce8, layout, 2, 12);
            let x = read::<f32>(start, 0) + width_at(curve[6]) + 1.;
            set_part(0x545c78, 11 - bonus, x);
            let color = if compared == 0 { 6 } else {
                match bonus.cmp(&handicraft(compared)) {
                    core::cmp::Ordering::Less => 3,
                    core::cmp::Ordering::Equal => 6,
                    core::cmp::Ordering::Greater => 10,
                }
            };
            let palette = read::<u32>(forge::mem::text_addr(), 0x1899458);
            gu!(0x56e6fc, panel, 2, 20, gu!(0x560e90, palette, color));
            gu!(0x56ffac, panel, 2, 20, gu!(0x55bc4c, messages, 1, 5), bonus);
        }
    }
}
#[forge::hook(offset = 0x5859f4)]
unsafe fn sharpness_detail(panel: u32, record: u32) -> u32 {
    if let Some(w) = unsafe { records::for_record(record) } {
        unsafe {
            let flags = read::<u32>(panel, 80);
            let aggregate = if flags & 0x8000 != 0 { read(panel, 184) } else { 0 };
            let compared = if flags & 0x4000 != 0 { read(panel, 188) } else { 0 };
            sharpness_bar(panel, w.level(), aggregate, compared);
        }
        return 0;
    }
    original!(panel, record)
}

#[forge::hook(offset = 0x56e810)]
unsafe fn equipment_name(panel: u32, group: u32, part: u32, record: u32, a: u32, b: u32, c: u32, flags: u32) -> u32 {
    let out = original!(panel, group, part, record, a, b, c, flags);
    if let Some(w) = unsafe { records::for_record(record) } {
        unsafe { gu!(0x56ea08, panel, group, part, w.definition.name.as_ptr() as u32); }
    }
    out
}
fn ranged(kind: u32) -> bool {
    if kind < mhgu_sdk::weapon::WeaponKind::COUNT as u32 {
        // GU's ranged types are heavy bowgun, light bowgun and bow.
        return matches!(kind, 4 | 6 | 10);
    }
    inventory::weapon_type(kind as u16).is_some_and(|(_, weapon)| weapon.ranged)
}

pub(in crate::weapon) mod grid;
const EXTENSION_TITLE: &CStr = c"扩展装备箱";

// GU's first page has nine real row widgets. Stock row 5 uses message
// Lb_ItemBoxMsg:5 (Manage Equipment). Insert below it and scroll the last
// row into view when all ten entries are present.
const INVENTORY_ROW: u32 = 6;
const HOME_ROWS: u32 = 9;

#[derive(Clone, Copy)]
struct HomeMenu {
    owner: u32,
    panel: u32,
    row: u32,
    stock_count: u32,
}

static HOME_MENU: Shared<Option<HomeMenu>> = Shared::new(None);

impl HomeMenu {
    fn stock_row(self) -> u32 {
        self.row - u32::from(self.row >= INVENTORY_ROW)
    }

    unsafe fn current(owner: u32) -> Option<Self> {
        unsafe {
            if read::<u32>(owner, 60) != 0 || read::<u32>(owner, 52) == 0 {
                return None;
            }
            let equipment = inventory::actor_equipment(local_hunter());
            if equipment == 0
                || profile::equipment_weapon(equipment) as usize >= crate::save::WEAPONS
            {
                return None;
            }
            let panel = gu!(0x65cb3c, owner, 0);
            if panel == 0 || read::<u32>(panel, 84) != owner + 300 || gu!(0x65cb3c, owner, 2) == 0 {
                return None;
            }
            // Panel +92 is the widget capacity. 714598 sets the actual
            // home row count on the cursor, including the unlockable ninth row.
            let stock_count = u32::from(read::<u8>(owner, 314));
            if !(INVENTORY_ROW..=HOME_ROWS).contains(&stock_count)
                || read::<u8>(owner + 300, 14) == 0
            {
                return None;
            }
            let saved = HOME_MENU.with(|home| *home);
            let row = saved
                .filter(|home| {
                    home.owner == owner && home.panel == panel && home.stock_count == stock_count
                })
                .map_or_else(
                    || {
                        let stock = read::<u32>(owner, 112).min(stock_count - 1);
                        stock + u32::from(stock >= INVENTORY_ROW)
                    },
                    |home| home.row,
                );
            Some(Self {
                owner,
                panel,
                row,
                stock_count,
            })
        }
    }

    unsafe fn apply(self) {
        HOME_MENU.with(|home| *home = Some(self));
        unsafe {
            set(self.owner, 112, self.stock_row());
            gu!(0x557d90, self.owner + 300, self.stock_row());
        }
    }
}

// Run the real cursor with one extra choice. Restore the retail row and count
// before 713DB8 reads them for hints or dispatches a stock command.
#[forge::hook(offset = 0x54ee98)]
unsafe fn navigate_home(cursor: u32, buttons: u32) -> u32 {
    let home = HOME_MENU.with(|home| *home);
    unsafe {
        let Some(mut home) =
            home.filter(|home| cursor == home.owner + 300 && read::<u32>(home.owner, 64) == 1)
        else {
            return original!(cursor, buttons);
        };
        let columns: u8 = read(cursor, 14);
        let count: u16 = read(cursor, 20);
        set(cursor, 14, (home.stock_count + 1) as u8);
        set(cursor, 20, (home.stock_count + 1) as u16);
        gu!(0x557d90, cursor, home.row);
        let moved = original!(cursor, buttons);
        home.row = gu!(0x557d78, cursor);
        set(cursor, 14, columns);
        set(cursor, 20, count);
        home.apply();
        moved
    }
}

#[forge::hook(offset = 0x713db8)]
unsafe fn equipment_box(ui: u32) -> u32 {
    unsafe {
        let phase: u32 = read(ui, 64);
        if phase == 0 || phase == 2 {
            HOME_DRAW.with(|state| *state = None);
            grid::close();
            HOME_MENU.with(|home| *home = None);
        }
        if phase != 1 {
            let out = original!(ui);
            if matches!(phase, 0 | 3 | 4)
                && read::<u32>(ui, 64) == 1
                && let Some(home) = HomeMenu::current(ui)
            {
                home.apply();
            }
            return out;
        }
        let Some(home) = HomeMenu::current(ui) else {
            HOME_MENU.with(|home| *home = None);
            return original!(ui);
        };
        HOME_MENU.with(|state| *state = Some(home));
        let pad: u32 = read(forge::mem::text_addr(), 0x188c670);
        let pressed = if pad == 0 { 0 } else { read::<u32>(pad, 27644) };
        if home.row != INVENTORY_ROW || pressed & (ACCEPT | BACK) != ACCEPT {
            return original!(ui);
        }

        home.apply();
        grid::open(ui, || original!(ui))
    }
}

#[derive(Clone, Copy)]
struct HomeDraw {
    panel: u32,
    first: u32,
    rows: u32,
    selected: u32,
    enabled: [u8; 9],
    submenu: bool,
}

static HOME_DRAW: Shared<Option<HomeDraw>> = Shared::new(None);

// Own the home page at the same boundary as GU's panel update. Its common
// list update (5C54BC) otherwise repaints the old selection before 5C2034.
#[forge::hook(offset = 0x5c1fa4)]
unsafe fn draw_home(panel: u32) -> u32 {
    unsafe {
        let Some(home) = HOME_MENU.with(|h| *h).filter(|h| {
            h.panel == panel
                && read::<u32>(panel, 84) == h.owner + 300
                && matches!(read::<u32>(h.owner, 64), 1 | 3)
                && read::<u32>(panel, 152) == 0
        }) else {
            HOME_DRAW.with(|state| {
                if state.is_some_and(|old| old.panel == panel) {
                    *state = None;
                }
            });
            return original!(panel);
        };
        let first = home.row.saturating_sub(HOME_ROWS - 1);
        let selected = home.row - first;
        let rows = (home.stock_count + 1).min(HOME_ROWS);
        let enabled = read::<[u8; 9]>(panel, 164);
        let submenu = read::<u32>(home.owner, 64) == 3;
        // GU also draws the child cursor and labels from this same panel.
        let out = if submenu { original!(panel) } else { 0 };
        let old = HOME_DRAW.with(|state| {
            state.replace(HomeDraw {
                panel,
                first,
                rows: home.stock_count,
                selected,
                enabled,
                submenu,
            })
        });
        let new_layout = old.is_none_or(|old| {
            old.panel != panel
                || old.first != first
                || old.rows != home.stock_count
                || old.submenu != submenu
        });
        if new_layout {
            // Retail initializes the title, prompts and child panels once.
            if !submenu {
                gu!(0x5c2034, panel);
            }
            gu!(0x5c5500, panel, rows);
        }
        // 5C56E8 positions the actual cursor sprite using the visible row.
        let cursor = home.owner + 300;
        let stock = read::<u8>(cursor, 13);
        set(cursor, 13, selected as u8);
        gu!(0x5c56e8, panel);
        set(cursor, 13, stock);
        for visible in 0..rows {
            let row = first + visible;
            let widget = read::<u32>(forge::mem::text_addr() + 0x16328f0, 4 * visible);
            let native = (row - u32::from(row > INVENTORY_ROW)) as usize;
            if new_layout || submenu {
                if row == INVENTORY_ROW {
                    gu!(0x56ea08, panel, widget, 1, EXTENSION_TITLE.as_ptr() as u32);
                } else {
                    let msg = read::<u8>(forge::mem::text_addr() + 0x1632974, native as u32);
                    gu!(0x56fc58, panel, widget, 1, 1, 22, u32::from(msg));
                }
            }
            if new_layout
                || submenu
                || old.is_some_and(|prev| {
                    (prev.selected == visible) != (selected == visible)
                        || row != INVENTORY_ROW && prev.enabled[native] != enabled[native]
                })
            {
                let available = if row == INVENTORY_ROW {
                    1
                } else {
                    enabled[native]
                };
                let color = gu!(
                    0x550354,
                    u32::from(available),
                    u32::from(selected == visible),
                    0,
                    0
                );
                gu!(0x56e6fc, panel, widget, 1, color);
            }
        }
        out
    }
}

#[forge::hook(offset = 0x62fb78)]
unsafe fn status_style(ui: u32, equipment: u32, details: u32) -> u32 {
    let out = original!(ui, equipment, details);
    unsafe {
        if let Some(entry) = profile::status_entry(equipment) {
            gu!(0x56e808, ui, 0x5c, 1, entry.name.as_ptr() as u32);
            let cursor: u32 = read(ui, 80);
            if details != 0 && cursor != 0 && read::<u8>(cursor, 13) == 0 {
                gu!(0x56e808, ui, 0x14, 1, entry.name.as_ptr() as u32);
                gu!(0x56e808, ui, 0x14, 2, entry.details.as_ptr() as u32);
            }
        }
    }
    out
}

#[forge::hook(offset = 0x633450)]
unsafe fn status_style_small(ui: u32) -> u32 {
    let out = original!(ui);
    unsafe {
        if let Some(entry) = profile::status_entry(read(ui, 76)) {
            gu!(0x56e808, ui, 0x69, 2, entry.name.as_ptr() as u32);
        }
    }
    out
}

// The My Set overview stores only the native style in its label widget.
// Keep the preview weapon with that widget until the next refresh or initialization.
static SET_PREVIEW: Shared<Option<(u32, u32, u32)>> = Shared::new(None);
static DETAIL_SET: Shared<Option<u32>> = Shared::new(None);

#[forge::hook(offset = 0x62e258)]
unsafe fn status_weapon(ui: u32) -> u32 {
    let out = original!(ui);
    unsafe {
        if let Some(weapon) = records::display(read(ui, 76)) {
            gu!(
                0x56e808,
                ui,
                0x27,
                2,
                weapon.definition.name.as_ptr() as u32
            );
            gu!(0x56fe88, ui, 0x27, 8, u32::from(weapon.upgrade) + 1, 0, 0);
        }
    }
    out
}

// cUIOStatusEquipWindow is also used for the equipment rows in My Set.
#[forge::hook(offset = 0x630268)]
unsafe fn equipment_overview(ui: u32) -> u32 {
    let out = original!(ui);
    unsafe {
        let equipment: u32 = read(ui, 76);
        if equipment != 0 {
            let weapon = match DETAIL_SET.with(|current| *current) {
                Some(slot) => sets::equipment_display(slot),
                None => records::display(equipment),
            };
            if let Some(weapon) = weapon {
                let entry = weapon.definition;
                gu!(0x56e808, ui, 5, 3, entry.name.as_ptr() as u32);
                icon::equipment(ui, 5, 1, entry);
                let slots = entry.levels[usize::from(weapon.upgrade)].slots;
                crate::weapon::decorations::draw(ui, 5, 4, slots, weapon.jewels);
            }
        }
    }
    out
}

// 674880 (pause menu) and 72AB38 (change equipment) pass a weapon record
// and separate cEquip copies for skill-adjusted values.
#[forge::hook(offset = 0x57f468)]
unsafe fn status_weapon_details(
    panel: u32,
    record: u32,
    compared: u32,
    equipment: u32,
    other: u32,
    flags: u32,
    player: u32,
    other_player: u32,
) -> u32 {
    let out = original!(
        panel,
        record,
        compared,
        equipment,
        other,
        flags,
        player,
        other_player
    );
    unsafe {
        let page = read::<u32>(panel, 248);
        if let Some(weapon) = records::for_record(record) && matches!(page, 0..=2 | 5) {
            let compare = flags & 0x4000 != 0 && other != 0;
            details_with_stats(
                panel,
                Some(weapon.definition),
                u32::from(weapon.upgrade) + 1,
                weapon.jewels,
                DetailStats {
                    equipment: if flags & 0x8000 != 0 { equipment } else { 0 },
                    compared_record: if compare { compared } else { 0 },
                    compared_equipment: if compare { other } else { 0 },
                },
            );
        }
    }
    out
}

#[forge::hook(offset = 0x589bc4)]
unsafe fn init_status_list(ui: u32) -> u32 {
    records::assign(ui + 80, None);
    original!(ui)
}

#[forge::hook(offset = 0x589d50)]
unsafe fn load_status_list(ui: u32, equipment_data: u32) -> u32 {
    let out = original!(ui, equipment_data);
    unsafe {
        let weapon = equipment_data
            .checked_sub(4)
            .and_then(|equip| records::display(equip));
        // 589C74 redraws from the panel's copy after consuming the UI revision.
        records::assign(ui + 80, weapon);
        if let Some(weapon) = weapon {
            let group = read(forge::mem::text_addr(), 0x163053c);
            gu!(0x56e808, ui, group, 2, weapon.definition.name.as_ptr() as u32);
            icon::equipment(ui, group, 1, weapon.definition);
        }
    }
    out
}

#[forge::hook(offset = 0x72d3d8)]
unsafe fn load_set_preview(ui: u32, slot: u32) -> u32 {
    SET_PREVIEW.with(|preview| *preview = None);
    let previous = DETAIL_SET.with(|current| current.replace(slot));
    let out = original!(ui, slot);
    DETAIL_SET.with(|current| *current = previous);
    unsafe {
        let save: u32 = read(forge::mem::text_addr(), 0x1856fe8);
        if read::<u32>(ui, 840) == 0 && gu!(0x146ba0, save, 0, slot) == 1 {
            let pane = gu!(0x65cbe8, ui, 22);
            if pane != 0 {
                let equipment = ui + 3052;
                SET_PREVIEW.with(|preview| *preview = Some((pane, slot, equipment)));
            }
        }
    }
    out
}

#[forge::hook(offset = 0x58a3dc)]
unsafe fn init_set_label(ui: u32) -> u32 {
    SET_PREVIEW.with(|preview| {
        if preview.is_some_and(|(pane, _, _)| pane == ui) {
            *preview = None;
        }
    });
    original!(ui)
}

#[forge::hook(offset = 0x58a44c)]
unsafe fn draw_set_label(ui: u32) -> u32 {
    let out = original!(ui);
    let preview = SET_PREVIEW.with(|preview| *preview);
    unsafe {
        if let Some((pane, slot, equipment)) = preview
            && ui == pane
            && read::<u32>(ui, 76) < 6
            && let Some(entry) = sets::preview(slot, equipment)
        {
            gu!(0x56ea08, ui, 0, 3, entry.name.as_ptr() as u32);
        }
    }
    out
}

#[forge::hook(offset = 0x5d96f8)]
unsafe fn draw_set_details(ui: u32, equipment: u32) -> u32 {
    let out = original!(ui, equipment);
    unsafe {
        if equipment != 0
            && let Some(slot) = DETAIL_SET.with(|current| *current)
            && let Some(entry) = sets::preview(slot, equipment)
        {
            let catalog: u32 = read(forge::mem::text_addr(), 0x1899220);
            let format = gu!(0x55bc4c, catalog, 1, 15);
            let title = gu!(0x55bc4c, catalog, 1, 63);
            gu!(
                0x56fd7c,
                ui,
                2,
                3,
                format,
                title,
                entry.name.as_ptr() as u32
            );
        }
    }
    out
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, equipment_box);
    forge::install_hook!(base, navigate_home);
    forge::install_hook!(base, draw_home);
    grid::install(base);
    forge::install_hook!(base, status_style);
    forge::install_hook!(base, status_style_small);
    forge::install_hook!(base, status_weapon);
    forge::install_hook!(base, sharpness_detail);
    forge::install_hook!(base, equipment_name);
    forge::install_hook!(base, equipment_overview);
    forge::install_hook!(base, status_weapon_details);
    forge::install_hook!(base, init_status_list);
    forge::install_hook!(base, load_status_list);
    forge::install_hook!(base, load_set_preview);
    forge::install_hook!(base, init_set_label);
    forge::install_hook!(base, draw_set_label);
    forge::install_hook!(base, draw_set_details);
}
