//! Equipment-box decorations for individually owned extension weapons.
use super::{gu, inventory, read};
use crate::save::CHARACTER;

pub(super) type Jewels = [u16; 3];

pub(super) fn installed(id: u32) -> Jewels {
    CHARACTER.with(|p| {
        p.inventory
            .decorations
            .get(&id)
            .copied()
            .unwrap_or_default()
    })
}

fn global(offset: u32) -> u32 {
    unsafe { read(forge::mem::text_addr(), offset) }
}

fn jewel_record(item: u16) -> Option<u32> {
    let items = global(0x18583a8);
    if item == 0 || items == 0 || unsafe { read::<u32>(items, 44) } == 0 {
        return None;
    }
    let row = unsafe { gu!(0x194230, items, u32::from(item)) };
    (row != 0).then_some(row)
}

pub(super) fn cost(item: u16) -> Option<u8> {
    let row = jewel_record(item)?;
    let slots = unsafe { read(row, 4) };
    (1..=3).contains(&slots).then_some(slots)
}

pub(super) fn refresh() {
    unsafe {
        let local = inventory::actor_equipment(super::local_hunter());
        let current = inventory::current_equipment();
        let recalculate = |eq| {
            // +396 is the skill table. DEE34 only needs +400 for gun ammunition.
            if eq != 0 && read::<u32>(eq, 396) != 0 {
                gu!(0x26fc24, eq);
            }
        };
        recalculate(local);
        if current != local {
            recalculate(current);
        }
    }
}

pub(super) unsafe fn draw(panel: u32, group: u32, first: u32, slots: u8, jewels: Jewels) {
    unsafe { draw_slots(panel, group, [first, first + 1, first + 2], slots, jewels) };
}

unsafe fn draw_slots(panel: u32, group: u32, parts: [u32; 3], slots: u8, jewels: Jewels) {
    unsafe {
        let mut i = 0;
        for id in jewels {
            let n = u32::from(cost(id).unwrap_or(0));
            for _ in 0..n.min(u32::from(slots) - i) {
                gu!(0x56eed0, panel, group, parts[i as usize], 1, u32::from(id));
                i += 1;
            }
        }
        for i in i..3 {
            gu!(
                0x56e6a8,
                panel,
                group,
                parts[i as usize],
                7,
                if i < u32::from(slots) { 2 } else { 0 }
            );
            gu!(
                0x56e6fc,
                panel,
                group,
                parts[i as usize],
                gu!(0x560e5c, 0, 0, 1)
            );
        }
    }
}

#[forge::hook(offset = 0x550570)]
unsafe fn record_slots(panel: u32, record: u32, group: u32, a: u32, b: u32, c: u32) -> u32 {
    if let Some(w) = unsafe { crate::weapon::records::for_record(record) } {
        unsafe {
            draw_slots(
                panel,
                group,
                [a, b, c],
                w.definition.levels[usize::from(w.upgrade)].slots,
                w.jewels,
            );
        }
        return 0;
    }
    original!(panel, record, group, a, b, c)
}

#[forge::hook(offset = 0xdb568)]
unsafe fn jewel_count(record: u32) -> u32 {
    unsafe { crate::weapon::records::for_record(record) }.map_or_else(
        || original!(record),
        |w| w.jewels.iter().filter(|&&id| id != 0).count() as u32,
    )
}

#[forge::hook(offset = 0xdb594)]
unsafe fn has_jewels(record: u32) -> bool {
    unsafe { crate::weapon::records::for_record(record) }
        .map_or_else(|| original!(record), |w| w.jewels != [0; 3])
}

#[forge::hook(offset = 0xdb66c)]
unsafe fn free_slots(record: u32) -> u32 {
    unsafe { crate::weapon::records::for_record(record) }.map_or_else(
        || original!(record),
        |w| {
            let used = w
                .jewels
                .into_iter()
                .filter(|&id| id != 0)
                .map(|id| cost(id).unwrap_or(3))
                .sum();
            w.definition.levels[usize::from(w.upgrade)]
                .slots
                .saturating_sub(used)
                .into()
        },
    )
}

#[forge::hook(offset = 0xdb7b8)]
unsafe fn jewel_row(record: u32, slot: u32) -> u32 {
    if let Some(w) = unsafe { crate::weapon::records::for_record(record) } {
        if let Some(&id) = w.jewels.get(slot as usize).filter(|&&id| id != 0) {
            return jewel_record(id).unwrap_or(0);
        }
        return 0;
    }
    original!(record, slot)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, record_slots);
    forge::install_hook!(base, jewel_count);
    forge::install_hook!(base, has_jewels);
    forge::install_hook!(base, free_slots);
    forge::install_hook!(base, jewel_row);
}
