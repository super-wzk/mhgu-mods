//! Supply SDK inventory records to GU's Change Equipment controller.
use super::{gu, inventory, ranged, read, set};
use alloc::vec::Vec;
use mhgu_sdk::{sync::Shared, weapon::WeaponInstance};

// Use GU's weapon category and a missing catalogue row for temporary UI copies.
const WEAPON_RECORD_TAG: u32 = 7;

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
    slots: Vec<u32>,
    smith: bool,
}

#[derive(Clone, Copy)]
enum Phase {
    Opening,
    Comparison,
    Input,
    Draw,
    Smith,
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

pub(in crate::weapon) fn close() {
    GRID.with(|grid| *grid = None);
}

fn owner_is(owner: u32) -> bool {
    GRID.with(|grid| grid.as_ref().is_some_and(|grid| grid.owner == owner))
}

fn row<T>(slot: u32, f: impl FnOnce(&Row) -> T) -> Option<T> {
    GRID.with(|grid| {
        let grid = grid.as_ref()?;
        let id = *grid.slots.get(slot as usize)?;
        grid.rows.iter().find(|r| r.instance.id == id).map(f)
    })
}

pub(super) unsafe fn open(owner: u32, native: impl FnOnce() -> u32) -> u32 {
    let panel = unsafe { gu!(0x65cb3c, owner, 2) };
    populate(owner, panel, false);
    let result = with_scope(owner, Phase::Opening, native);
    if unsafe { read::<u32>(owner, 64) } == 3 { unsafe { weapon_only_rows(owner); } } else {
        close();
    }
    result
}

fn populate(owner: u32, panel: u32, smith: bool) {
    let rows: Vec<Row> = inventory::inventory()
        .into_iter()
        .filter_map(|(instance, entry)| {
            let (_, equipment) = inventory::definition(instance.equipment.get())?;
            let mut record = [0; 11];
            // DADF0 copies bytes 4..40 into the native comparison and socket panels.
            record[1] = WEAPON_RECORD_TAG | (0xffff << 16);
            record[9] = u32::from_le_bytes(*b"FZBX");
            record[7] = u32::from(instance.equipment.get());
            record[8] = u32::from(instance.upgrade);
            let [a, b, c] = crate::weapon::decorations::installed(instance.id);
            record[2] = u32::from(a) << 16;
            record[3] = u32::from(b) | u32::from(c) << 16;
            Some(Row {
                record,
                instance,
                equipment,
                ranged: ranged(entry.style.weapon),
            })
        })
        .collect();
    let slots = if smith { rows.iter().map(|r| r.instance.id).collect() } else { inventory::box_slots() };
    GRID.with(|grid| *grid = Some(Grid { owner, panel, rows, slots, smith }));
}

pub(in crate::weapon) unsafe fn open_smith(owner: u32) {
    populate(owner, unsafe { gu!(0x65cb3c, owner, 8) }, true);
}

pub(in crate::weapon) fn smith_open(owner: u32) -> bool {
    GRID.with(|g| g.as_ref().is_some_and(|g| g.owner == owner && g.smith))
}

pub(in crate::weapon) fn smith_active(owner: u32) -> bool {
    scope().is_some_and(|s| s.owner == owner && matches!(s.phase, Phase::Smith))
}

pub(in crate::weapon) fn with_smith<T>(owner: u32, f: impl FnOnce() -> T) -> T {
    with_scope(owner, Phase::Smith, f)
}

pub(in crate::weapon) fn smith_instance(slot: u32) -> Option<WeaponInstance> {
    let id = row(slot, |r| r.instance.id)?;
    inventory::owned_equipment(id).map(|(item, _)| item)
}

pub(in crate::weapon) fn upgraded(item: WeaponInstance) {
    GRID.with(|grid| {
        if let Some(row) = grid.as_mut().and_then(|g| g.rows.iter_mut().find(|r| r.instance.id == item.id)) {
            row.instance = item;
            row.record[8] = u32::from(item.upgrade);
        }
    });
}

mod management;

unsafe fn weapon_only_rows(owner: u32) {
    let home = unsafe { gu!(0x65cb3c, owner, 0) };
    if home != 0 { unsafe { set(home, 174, 0u8); set(home, 176, 0u8); } }
}

#[forge::hook(offset = 0x718084)]
unsafe fn update_child(owner: u32) -> u32 {
    if !owner_is(owner) { return original!(owner); }
    unsafe {
        let submenu = read::<u32>(owner, 88) == 1;
        let kind = read::<u32>(owner, 132);
        let pad = read::<u32>(forge::mem::text_addr(), 0x188c670);
        let accept = pad != 0 && read::<u32>(pad, 27644) & 0x2000 != 0;
        if submenu && accept && matches!(kind, 1 | 3) {
            gu!(0x54f260, 0, 3);
            return 1;
        }
        let phase = if submenu && accept && kind == 0 { Phase::Opening } else { Phase::Input };
        let result = with_scope(owner, phase, || original!(owner));
        if result == 0 { close(); }
        else if read::<u32>(owner, 88) == 1 { weapon_only_rows(owner); }
        else if submenu && accept && kind == 0 && let Some((equipped, _)) = inventory::selected()
            && let Some(slot) = GRID.with(|g| g.as_ref()?.slots.iter().position(|&id| id == equipped.id)) {
            set(owner, 392, (slot % 10) as u8);
            set(owner, 393, (slot / 10 % 10) as u8);
            set(owner, 396, (slot / 100) as u16);
            set(owner, 824, slot as u32);
            with_scope(owner, Phase::Input, || gu!(0x72ab38, owner, 0, 0));
        }
        result
    }
}

pub(in crate::weapon) unsafe fn projected(record: u32) -> Option<crate::weapon::records::DisplayEquipment> {
    if record == 0 || scope().is_none() { return None; }
    unsafe {
        if read::<u16>(record, 4) & 31 != WEAPON_RECORD_TAG as u16
            || read::<u32>(record, 36) != u32::from_le_bytes(*b"FZBX") { return None; }
        let mut w = crate::weapon::records::DisplayEquipment::registered(read(record, 28), read(record, 32))?;
        w.jewels = read(record, 10);
        Some(w)
    }
}

unsafe extern "C" fn box_size(save: u32, kind: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        if let Some(s) = scope() && !smith_open(s.owner) && unsafe { read::<u32>(s.owner, 132) } != 0 {
            return crate::save::INVENTORY_LIMIT as u32;
        }
        return GRID.with(|grid| grid.as_ref().map_or(0, |grid| grid.slots.len() as u32));
    }
    // 1418E4 loads the player pointer through a PC-relative literal.
    let (base_slots, slots_per_unlock) = match kind {
        0 => (1000, 200),
        1 => (300, 100),
        _ => return 0,
    };
    unsafe {
        let player = read::<u32>(forge::mem::text_addr(), 0x1897f78);
        let flags = read::<u32>(player, 2392);
        base_slots + (flags & (3 << 21)).count_ones() * 200 + gu!(0x525878, player) * slots_per_unlock
    }
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
    if kind == 0 && let Some(scope) = for_save(save) {
        if matches!(scope.phase, Phase::Comparison) {
            // Native comparison reads only the retail weapon category here.
            let current = unsafe { inventory::current_equipment() };
            return if current != 0 && row(slot, |_| ()).is_some() { current + 24 } else { 0 };
        }
        static EMPTY: [u32; 11] = [0; 11];
        let empty = if !smith_open(scope.owner) && unsafe { read::<u32>(scope.owner, 132) } != 0 && slot < crate::save::INVENTORY_LIMIT as u32 {
            EMPTY.as_ptr() as u32
        } else { 0 };
        return row(slot, |r| r.record.as_ptr() as u32).unwrap_or(empty);
    }
    original!(save, kind, slot)
}

#[forge::hook(offset = 0x144c04)]
unsafe fn can_equip(save: u32, slot: u32) -> u32 {
    if for_save(save).is_some() {
        let equipped = inventory::selected().map(|(item, _)| item.id);
        return u32::from(row(slot, |r| Some(r.instance.id) != equipped) == Some(true));
    }
    // GU 144C04 rejects its current weapon before testing weapon compatibility.
    // A return to that same physical record is a real change from an SDK weapon.
    if slot < 2000 && save != 0 && inventory::selected().is_some() {
        unsafe {
            let manager = read::<u32>(forge::mem::text_addr(), 0x1885b58);
            let eq = gu!(0x277454, manager);
            if (7..=21).contains(&(read::<u8>(save, 32 + slot * 44) & 31))
                && eq != 0 && gu!(0xe100c, eq + 4, 0) == slot
                && gu!(0x276278, manager) != 0 {
                return 1;
            }
        }
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

// GU also calls 144590 before selling or mutating the physical backing slot.
// Keep those checks physical and resolve the visible mark at its UI callers.
#[unsafe(naked)]
unsafe extern "C" fn equipped(_save: u32, _slot: u32) -> u32 {
    core::arch::naked_asm!("mov r2, lr", "b {query}", query = sym equipped_at);
}

unsafe extern "C" fn equipped_at(save: u32, slot: u32, caller: u32) -> u32 {
    if for_save(save).is_some() {
        let equipped = inventory::selected().map(|(item, _)| item.id);
        return u32::from(row(slot, |r| Some(r.instance.id) == equipped) == Some(true));
    }
    // 144594 uses the same literal-load pattern to reach the hunter manager.
    unsafe {
        let manager = read::<u32>(forge::mem::text_addr(), 0x1885b58);
        let equipment = gu!(0x277454, manager);
        if equipment == 0 { return 0; }
        let weapon = gu!(0xe100c, equipment + 4, 0) == slot;
        if weapon {
            let ui = matches!(caller.wrapping_sub(forge::mem::text_addr()),
                0x5b6db4 | 0x5b6ebc | 0x5b9330 | 0x71f3b4 | 0x720d90 | 0x72abb8);
            return u32::from(!ui || inventory::selected().is_none());
        }
        u32::from((1..7).any(|part| gu!(0xe100c, equipment + 4, part) == slot))
    }
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
unsafe extern "C" fn build_sort(save: u32, kind: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return 0;
    }
    // 149360 branches relative to the entry. Call each original sort directly.
    unsafe {
        match kind {
            0 => gu!(0x142268, save, kind),
            1 => gu!(0x149374, save, kind),
            _ => save,
        }
    }
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
                let mut sorted = grid.rows.iter().map(|r| r.instance.id);
                for id in &mut grid.slots {
                    if *id == 0 || grid.rows.iter().any(|r| r.instance.id == *id) {
                        *id = sorted.next().unwrap_or(0);
                    }
                }
                if !grid.smith { inventory::save_slots(grid.slots.clone()); }
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
        let current = unsafe { inventory::current_equipment() };
        if row(slot, |_| ()).is_none() || current == 0 || unsafe { read::<u32>(current, 396) } == 0 {
            unsafe {
                gu!(0x54edec, owner + 444, 5, 0, 0);
                gu!(0x54ee34, owner + 444, 0, 6);
            }
            return 0;
        }
        return with_scope(owner, Phase::Comparison, || original!(owner, slot));
    }
    original!(owner, slot)
}

// 1446B8 normally rebuilds jewel points in the retail inventory cell by index.
// The extension candidate has a different inventory and is evaluated on its copy below.
#[forge::hook(offset = 0x1446b8)]
unsafe fn refresh_cell(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return u32::from(row(slot, |_| ()).is_some());
    }
    original!(save, kind, slot)
}

// GU makes an independent cEquip for each side of its skill comparison.
#[forge::hook(offset = 0x72c6cc)]
unsafe fn compare_weapon(arg: u32, copy: u32, slot: u32) -> u32 {
    if scope().is_some_and(|s| matches!(s.phase, Phase::Comparison)) {
        if let Some(item) = row(slot, |r| r.instance) {
            crate::weapon::records::remember(copy, crate::weapon::records::DisplayEquipment::owned(item));
            unsafe {
                gu!(0x26e580, copy, 1);
                gu!(0x26fc24, copy);
                return gu!(0x4d0720, read::<u32>(forge::mem::text_addr(), 0x1896c20),
                    gu!(0x3f8398, read::<u32>(forge::mem::text_addr(), 0x18843f0)), copy);
            }
        }
        return 0;
    }
    original!(arg, copy, slot)
}

#[forge::hook(offset = 0x72ab38)]
unsafe fn details(owner: u32, kind: u32, phase: u32) -> u32 {
    if kind != 0 || !scope().is_some_and(|s| s.owner == owner) || phase != 0 {
        return original!(owner, kind, phase);
    }
    unsafe {
        let current = inventory::current_equipment();
        if current != 0 && read::<u32>(current, 396) != 0 {
            return with_scope(owner, Phase::Comparison, || original!(owner, kind, phase));
        }
        let candidate = row(read(owner, 824), |r| (r.equipment, r.instance));
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
                    crate::weapon::decorations::installed(item.id),
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
            candidate.map_or(0, |(_, item)| u32::from(item.upgrade) + 1),
            candidate.map_or([0; 3], |(_, item)| crate::weapon::decorations::installed(item.id)),
        );
    }
    0
}

fn row_display(panel: u32, record: u32) -> Option<(inventory::RegisteredEquipment, u16)> {
    if !matches!(scope()?.phase, Phase::Draw) {
        return None;
    }
    GRID.with(|grid| {
        let grid = grid.as_ref().filter(|grid| grid.panel == panel)?;
        let begin = grid.rows.as_ptr() as u32;
        let at = record.checked_sub(begin)? as usize / core::mem::size_of::<Row>();
        let row = grid.rows.get(at)?;
        (row.record.as_ptr() as u32 == record)
            .then_some((row.equipment, row.instance.upgrade))
    })
}

#[forge::hook(offset = 0x56ea38)]
unsafe fn icon(panel: u32, group: u32, part: u32, record: u32, style: u32, flag: u32) -> u32 {
    if let Some((entry, _)) = row_display(panel, record) {
        return unsafe { super::super::icon::equipment(panel, group, part, entry) };
    }
    if let Some(w) = unsafe { crate::weapon::records::for_record(record) } {
        return unsafe { crate::weapon::icon::equipment(panel, group, part, w.definition) };
    }
    original!(panel, group, part, record, style, flag)
}

#[forge::hook(offset = 0x5b973c)]
unsafe fn name(panel: u32, record: u32, kind: u32) -> u32 {
    if let Some((entry, upgrade)) = row_display(panel, record) {
        let (count, room) = crate::weapon::menu::owned_count(entry.equipment.id.get(), upgrade);
        return unsafe { gu!(0x5b8f38, panel, entry.name.as_ptr() as u32, 1, count, u32::from(room), 0) };
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
    with_scope(owner, Phase::Draw, || original!(panel))
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
    management::install(base);
    forge::install_hook!(base, update_child);
    for (offset, replacement) in [
        (0x1418e0, box_size as unsafe extern "C" fn(u32, u32) -> u32),
        (0x144590, equipped),
        (0x14935c, build_sort),
    ] {
        unsafe {
            forge::sys::hook::forge_hook_create(
                (base + offset) as *const _, replacement as *const _, core::ptr::null_mut(),
            );
        }
    }
    forge::install_hook!(base, cell);
    forge::install_hook!(base, selection);
    forge::install_hook!(base, can_equip);
    forge::install_hook!(base, category);
    forge::install_hook!(base, occupied);
    forge::install_hook!(base, equipment_type);
    forge::install_hook!(base, compatible);
    forge::install_hook!(base, can_remove);
    forge::install_hook!(base, weapon_class);
    forge::install_hook!(base, apply_sort);
    forge::install_hook!(base, stock_box_view);
    forge::install_hook!(base, skill_cursor);
    forge::install_hook!(base, compare_weapon);
    forge::install_hook!(base, refresh_cell);
    forge::install_hook!(base, details);
    forge::install_hook!(base, icon);
    forge::install_hook!(base, name);
    forge::install_hook!(base, draw);
    forge::install_hook!(base, draw_sort);
}
