//! Supply SDK inventory records to GU's Change Equipment controller.
use super::{EXTENSION_TITLE, gu, inventory, ranged, read, set};
use alloc::vec::Vec;
use mhgu_sdk::{sync::Shared, weapon::WeaponInstance};

// The grid and the switch branch only need a non-empty, non-armour tag.
const WEAPON_RECORD_TAG: u32 = 31;

#[repr(C)]
struct Row {
    record: [u32; 11],
    instance: WeaponInstance,
    equipment: inventory::RegisteredEquipment,
    ranged: bool,
}

struct Grid {
    owner: u32,
    panel: u32,
    rows: Vec<Row>,
}

#[derive(Clone, Copy)]
enum Phase {
    Opening,
    Input,
    Draw,
}

#[derive(Clone, Copy)]
struct Scope {
    owner: u32,
    save: u32,
    thread: u32,
    phase: Phase,
    compared_current: bool,
}

static GRID: Shared<Option<Grid>> = Shared::new(None);
static SCOPE: Shared<Option<Scope>> = Shared::new(None);

fn thread_id() -> u32 {
    let current: unsafe extern "C" fn() -> u32 =
        unsafe { core::mem::transmute(forge::mem::text_addr() + 0x13effd4) };
    unsafe { current() }
}

fn scope() -> Option<Scope> {
    SCOPE
        .with(|scope| *scope)
        .filter(|scope| scope.thread == thread_id())
}

fn for_save(save: u32) -> Option<Scope> {
    scope().filter(|scope| scope.save == save)
}

fn with_scope<T>(owner: u32, phase: Phase, native: impl FnOnce() -> T) -> T {
    let save = unsafe { read(forge::mem::text_addr(), 0x1856fe8) };
    let thread = thread_id();
    let previous = SCOPE.with(|scope| {
        scope.replace(Scope {
            owner,
            save,
            thread,
            phase,
            compared_current: false,
        })
    });
    let result = native();
    SCOPE.with(|scope| *scope = previous);
    result
}

pub(super) fn close() {
    GRID.with(|grid| *grid = None);
}

fn owner_is(owner: u32) -> bool {
    GRID.with(|grid| grid.as_ref().is_some_and(|grid| grid.owner == owner))
}

fn row<T>(slot: u32, f: impl FnOnce(&Row) -> T) -> Option<T> {
    GRID.with(|grid| grid.as_ref()?.rows.get(slot as usize).map(f))
}

pub(super) unsafe fn open(owner: u32, native: impl FnOnce() -> u32) -> u32 {
    let panel = unsafe { gu!(0x65cb3c, owner, 2) };
    let rows = inventory::inventory()
        .into_iter()
        .filter_map(|(instance, entry)| {
            let (_, equipment) = inventory::definition(instance.equipment.get())?;
            let mut record = [0; 11];
            // GU's read-only record header tags a weapon and its item row.
            // The SDK ID never enters the native catalogue or save.
            record[1] = WEAPON_RECORD_TAG | (u32::from(instance.equipment.get()) << 16);
            Some(Row {
                record,
                instance,
                equipment,
                ranged: ranged(entry.style.weapon),
            })
        })
        .collect();
    GRID.with(|grid| *grid = Some(Grid { owner, panel, rows }));
    let result = with_scope(owner, Phase::Opening, native);
    if unsafe { read::<u32>(owner, 64) } != 3 {
        close();
    }
    result
}

// The extension occupies a separate home row and enters the same equipment
// child that GU opens from Manage Equipment → Change Equipment.
#[forge::hook(offset = 0x714b20)]
unsafe fn open_child(owner: u32) -> u32 {
    if !scope().is_some_and(|s| s.owner == owner && matches!(s.phase, Phase::Opening)) {
        return original!(owner);
    }
    unsafe {
        set(owner, 132, 0u32);
        gu!(0x71f108, owner);
        gu!(0x65cbc4, owner, 0);
        set(owner, 88, 2u32);

        if let Some((equipped, _)) = inventory::selected()
            && let Some(slot) = GRID.with(|grid| {
                grid.as_ref()?
                    .rows
                    .iter()
                    .position(|r| r.instance.id == equipped.id)
            })
        {
            set(owner, 392, (slot % 10) as u8);
            set(owner, 393, (slot / 10 % 10) as u8);
            set(owner, 396, (slot / 100) as u16);
        }
    }
    0
}

#[forge::hook(offset = 0x718084)]
unsafe fn update_child(owner: u32) -> u32 {
    if !owner_is(owner) {
        return original!(owner);
    }
    let result = with_scope(owner, Phase::Input, || unsafe { gu!(0x7201cc, owner) });
    if result == 0 {
        // Return through GU's home transition. The extension has no submenu.
        close();
        unsafe {
            set(owner, 88, 0u32);
            gu!(0x54f260, 0, 2);
        }
    }
    result
}

#[forge::hook(offset = 0x1418e0)]
unsafe fn box_size(save: u32, kind: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return GRID.with(|grid| grid.as_ref().map_or(0, |grid| grid.rows.len() as u32));
    }
    original!(save, kind)
}

#[forge::hook(offset = 0x14394c)]
unsafe fn cell(save: u32, kind: u32, page: u32, slot: u32) -> u32 {
    if kind == 0
        && let Some(scope) = for_save(save)
    {
        // The native initializer's actor comparison is for retail equipment.
        if matches!(scope.phase, Phase::Opening) {
            return 0;
        }
        return row(page * 100 + slot, |r| r.record.as_ptr() as u32).unwrap_or(0);
    }
    original!(save, kind, page, slot)
}

#[forge::hook(offset = 0x143894)]
unsafe fn selection(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return row(slot, |r| r.record.as_ptr() as u32).unwrap_or(0);
    }
    original!(save, kind, slot)
}

#[forge::hook(offset = 0x144c04)]
unsafe fn can_equip(save: u32, slot: u32) -> u32 {
    if for_save(save).is_some() {
        let equipped = inventory::selected().map(|(item, _)| item.id);
        return u32::from(row(slot, |r| Some(r.instance.id) != equipped) == Some(true));
    }
    original!(save, slot)
}

#[forge::hook(offset = 0x1440c8)]
unsafe fn category(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return if row(slot, |_| ()).is_some() { 0 } else { 6 };
    }
    original!(save, kind, slot)
}

#[forge::hook(offset = 0x144190)]
unsafe fn occupied(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return u32::from(row(slot, |_| ()).is_some());
    }
    original!(save, kind, slot)
}

#[forge::hook(offset = 0x141944)]
unsafe fn equipment_type(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return row(slot, |r| r.record[1] & 31).unwrap_or(0);
    }
    original!(save, kind, slot)
}

#[forge::hook(offset = 0x144590)]
unsafe fn equipped(save: u32, slot: u32) -> u32 {
    if for_save(save).is_some() {
        let equipped = inventory::selected().map(|(item, _)| item.id);
        return u32::from(row(slot, |r| Some(r.instance.id) == equipped) == Some(true));
    }
    original!(save, slot)
}

#[forge::hook(offset = 0x1449b4)]
unsafe fn compatible(save: u32, slot: u32) -> u32 {
    if for_save(save).is_some() {
        return u32::from(row(slot, |_| ()).is_some());
    }
    original!(save, slot)
}

#[forge::hook(offset = 0x144b68)]
unsafe fn can_remove(save: u32, slot: u32) -> u32 {
    if for_save(save).is_some() {
        return 0;
    }
    original!(save, slot)
}

#[forge::hook(offset = 0x145b58)]
unsafe fn weapon_class(save: u32, slot: u32) -> u32 {
    if let Some(s) = for_save(save)
        && matches!(s.phase, Phase::Input)
        && unsafe { read::<u32>(s.owner, 136) } == 1
    {
        // GU's confirmation compares the equipped weapon, then the target.
        if s.compared_current {
            return row(slot, |r| u32::from(r.ranged)).unwrap_or(255);
        }
        SCOPE.with(|scope| scope.as_mut().unwrap().compared_current = true);
        if let Some((_, entry)) = inventory::selected() {
            return u32::from(ranged(entry.style.weapon));
        }
    }
    original!(save, slot)
}

pub(in crate::weapon) fn equip(save: u32, slot: u32) -> Option<u32> {
    let scope = for_save(save)?;
    if !matches!(scope.phase, Phase::Input) || unsafe { read::<u32>(scope.owner, 136) } != 3 {
        return None;
    }
    Some(u32::from(
        row(slot, |r| r.instance.id).is_some_and(|id| inventory::select(Some(id))),
    ))
}

// GU prepares and applies its stock sort through these save entry points.
#[forge::hook(offset = 0x14935c)]
unsafe fn build_sort(save: u32, kind: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return 0;
    }
    original!(save, kind)
}

#[forge::hook(offset = 0x1424ec)]
unsafe fn apply_sort(save: u32, kind: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        GRID.with(|grid| {
            if let Some(grid) = grid.as_mut() {
                grid.rows.sort_unstable_by_key(|r| {
                    (
                        r.equipment.equipment.weapon.get(),
                        r.instance.equipment.get(),
                        r.instance.upgrade,
                        r.instance.id,
                    )
                });
            }
        });
        return 0;
    }
    original!(save, kind)
}

#[forge::hook(offset = 0x149e6c)]
unsafe fn stock_box_view(save: u32, kind: u32, mode: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return 0;
    }
    original!(save, kind, mode, slot)
}

#[forge::hook(offset = 0x72aa48)]
unsafe fn skill_cursor(owner: u32, slot: u32) -> u32 {
    if scope().is_some_and(|s| s.owner == owner) {
        unsafe {
            gu!(0x54edec, owner + 444, 5, 0, 0);
            gu!(0x54ee34, owner + 444, 0, 6);
        }
        return 0;
    }
    original!(owner, slot)
}

#[forge::hook(offset = 0x72ab38)]
unsafe fn details(owner: u32, kind: u32, phase: u32) -> u32 {
    if kind != 0 || !scope().is_some_and(|s| s.owner == owner) || phase != 0 {
        return original!(owner, kind, phase);
    }
    unsafe {
        let candidate = row(read(owner, 824), |r| (r.equipment, r.instance.upgrade));
        if candidate.is_none() {
            gu!(0x65cc0c, owner, 3);
            gu!(0x65cc0c, owner, 4);
            return 0;
        }
        let current_panel = gu!(0x65cbe8, owner, 3);
        let panel = gu!(0x65cbe8, owner, 4);
        if current_panel != 0 && panel != 0 {
            if let Some((item, _)) = inventory::selected()
                && let Some((_, equipment)) = inventory::definition(item.equipment.get())
            {
                crate::weapon::menu::details(
                    current_panel,
                    Some(equipment),
                    u32::from(item.upgrade) + 1,
                );
            } else {
                let equipment = inventory::local_equipment();
                let record = if equipment == 0 { 0 } else { equipment + 24 };
                gu!(0x57f468, current_panel, record, 0, 0, 0, 0x8000, 0, 0);
            }
        }
        crate::weapon::menu::details(
            if panel == 0 { current_panel } else { panel },
            candidate.map(|(e, _)| e),
            candidate.map_or(0, |(_, level)| u32::from(level) + 1),
        );
    }
    0
}

fn row_display(panel: u32, record: u32) -> Option<(inventory::RegisteredEquipment, u32)> {
    if !matches!(scope()?.phase, Phase::Draw) {
        return None;
    }
    GRID.with(|grid| {
        let grid = grid.as_ref().filter(|grid| grid.panel == panel)?;
        let begin = grid.rows.as_ptr() as u32;
        let at = record.checked_sub(begin)? as usize / core::mem::size_of::<Row>();
        let row = grid.rows.get(at)?;
        (row.record.as_ptr() as u32 == record)
            .then_some((row.equipment, u32::from(row.instance.upgrade) + 1))
    })
}

#[forge::hook(offset = 0x56ea38)]
unsafe fn icon(panel: u32, group: u32, part: u32, record: u32, style: u32, flag: u32) -> u32 {
    if let Some((entry, _)) = row_display(panel, record) {
        return unsafe { super::super::icon::equipment(panel, group, part, entry) };
    }
    original!(panel, group, part, record, style, flag)
}

#[forge::hook(offset = 0x5b973c)]
unsafe fn name(panel: u32, record: u32, kind: u32) -> u32 {
    if let Some((entry, level)) = row_display(panel, record) {
        return unsafe { gu!(0x5b8f38, panel, entry.name.as_ptr() as u32, 1, level, 1, 0) };
    }
    original!(panel, record, kind)
}

fn panel_owner(panel: u32) -> Option<u32> {
    GRID.with(|grid| {
        grid.as_ref()
            .filter(|grid| grid.panel == panel)
            .map(|grid| grid.owner)
    })
}

#[forge::hook(offset = 0x5b91e0)]
unsafe fn draw(panel: u32) -> u32 {
    let Some(owner) = panel_owner(panel) else {
        return original!(panel);
    };
    with_scope(owner, Phase::Draw, || unsafe {
        let result = original!(panel);
        gu!(0x56ea08, panel, 4, 1, EXTENSION_TITLE.as_ptr() as u32);
        result
    })
}

#[forge::hook(offset = 0x5b6c78)]
unsafe fn draw_sort(panel: u32, kind: u32) -> u32 {
    if panel_owner(panel).is_some() {
        // The alternate native renderer reads a stock sort buffer directly.
        return unsafe { gu!(0x5b91e0, panel) };
    }
    original!(panel, kind)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, open_child);
    forge::install_hook!(base, update_child);
    forge::install_hook!(base, box_size);
    forge::install_hook!(base, cell);
    forge::install_hook!(base, selection);
    forge::install_hook!(base, can_equip);
    forge::install_hook!(base, category);
    forge::install_hook!(base, occupied);
    forge::install_hook!(base, equipment_type);
    forge::install_hook!(base, equipped);
    forge::install_hook!(base, compatible);
    forge::install_hook!(base, can_remove);
    forge::install_hook!(base, weapon_class);
    forge::install_hook!(base, build_sort);
    forge::install_hook!(base, apply_sort);
    forge::install_hook!(base, stock_box_view);
    forge::install_hook!(base, skill_cursor);
    forge::install_hook!(base, details);
    forge::install_hook!(base, icon);
    forge::install_hook!(base, name);
    forge::install_hook!(base, draw);
    forge::install_hook!(base, draw_sort);
}
