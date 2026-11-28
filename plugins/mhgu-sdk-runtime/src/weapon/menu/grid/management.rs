//! Inventory operations called by GU's sale, arrange and decoration controllers.
use super::{GRID, box_size, for_save, gu, inventory, projected, row, set};
use super::{panel_owner, read, scope};
use crate::{
    save::{CHARACTER, INVENTORY_LIMIT},
    weapon::{decorations, records, sets, trade},
};
use mhgu_sdk::weapon::WeaponInstance;

fn item(slot: u32) -> Option<WeaponInstance> {
    row(slot, |r| r.instance)
}

#[forge::hook(offset = 0x1437e0)]
unsafe fn price(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return item(slot).and_then(trade::sale_price).unwrap_or(u32::MAX);
    }
    original!(save, kind, slot)
}

#[forge::hook(offset = 0x143390)]
unsafe fn sell(save: u32, kind: u32, slot: u32) -> u32 {
    if kind != 0 || for_save(save).is_none() {
        return original!(save, kind, slot);
    }
    let Some(item) = item(slot) else { return 0 };
    let Some(price) = trade::sale_price(item) else {
        return 0;
    };
    if !trade::sell(item, price) {
        return 0;
    }
    GRID.with(|g| {
        if let Some(grid) = g {
            grid.slots[slot as usize] = 0;
            inventory::save_slots(grid.slots.clone());
            grid.rows.retain(|r| r.instance.id != item.id);
        }
    });
    price
}

#[forge::hook(offset = 0x14426c)]
unsafe fn eligibility(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        let Some(item) = item(slot) else { return 3 };
        if inventory::selected().is_some_and(|(w, _)| w.id == item.id) {
            return 1;
        }
        return if trade::sale_price(item).is_some() {
            0
        } else {
            2
        };
    }
    // 14426C has a PC-relative prologue. Use the complete GU entry for the
    // equipped check and the physical record in the global save for the rest.
    unsafe {
        if slot == 0xffff || slot >= box_size(save, kind) {
            return 3;
        }
        if gu!(0x1444b4, save, kind, slot) != 0 {
            return 1;
        }
        let native_save = read::<u32>(forge::mem::text_addr(), 0x1856fe8);
        let first = if kind == 0 { 28 } else { 88028 };
        let record = native_save + first + 44 * slot;
        if read::<u8>(record, 4) & 31 == 0 {
            return 3;
        }
        2 * gu!(0xdb070, record)
    }
}

#[forge::hook(offset = 0x148340)]
unsafe fn set_index(save: u32, kind: u32, slot: u32) -> u32 {
    if kind == 0 && for_save(save).is_some() {
        return item(slot)
            .and_then(|item| {
                CHARACTER.with(|p| {
                    p.inventory
                        .sets
                        .iter()
                        .rev()
                        .find_map(|(&s, link)| (link.instance == item.id).then_some(u32::from(s)))
                })
            })
            .unwrap_or(255);
    }
    // The native prologue branches PC-relatively and Recomp needs full entry points.
    unsafe { sets::stock_set_index(save, kind, slot) }
}

#[forge::hook(offset = 0x143a14)]
unsafe fn swap(save: u32, kind: u32, src: u32, dst: u32) -> u32 {
    if kind != 0 || for_save(save).is_none() {
        return original!(save, kind, src, dst);
    }
    if src.max(dst) as usize >= INVENTORY_LIMIT {
        return 0;
    }
    GRID.with(|g| {
        let Some(grid) = g else { return 0 };
        grid.slots
            .resize(grid.slots.len().max(src.max(dst) as usize + 1), 0);
        grid.slots.swap(src as usize, dst as usize);
        inventory::save_slots(grid.slots.clone());
        1
    })
}

fn fits(slot: u32, replace: Option<usize>, jewel: u32) -> bool {
    let Some(w) = item(slot).and_then(records::DisplayEquipment::owned) else {
        return false;
    };
    let Ok(jewel) = u16::try_from(jewel) else {
        return false;
    };
    let Some(cost) = decorations::cost(jewel) else {
        return false;
    };
    let used: u16 = w
        .jewels
        .iter()
        .enumerate()
        .filter(|(i, id)| Some(*i) != replace && **id != 0)
        .map(|(_, id)| u16::from(decorations::cost(*id).unwrap_or(3)))
        .sum();
    u16::from(cost) + used <= u16::from(w.level().slots)
}

#[forge::hook(offset = 0x144dac)]
unsafe fn sockets(save: u32, slot: u32) -> bool {
    if for_save(save).is_some() {
        return item(slot)
            .and_then(records::DisplayEquipment::owned)
            .is_some_and(|w| w.level().slots != 0);
    }
    original!(save, slot)
}
#[forge::hook(offset = 0x144f94)]
unsafe fn fits_empty(save: u32, slot: u32, jewel: u32) -> bool {
    if for_save(save).is_some() {
        return fits(slot, None, jewel);
    }
    original!(save, slot, jewel)
}
#[forge::hook(offset = 0x1450f0)]
unsafe fn fits_replacing(save: u32, slot: u32, index: u32, jewel: u32) -> bool {
    if for_save(save).is_some() {
        return fits(slot, Some(index as usize), jewel);
    }
    original!(save, slot, index, jewel)
}

fn write(item: WeaponInstance, jewels: decorations::Jewels) {
    CHARACTER.with(|p| {
        if jewels == [0; 3] {
            p.inventory.decorations.remove(&item.id);
        } else {
            p.inventory.decorations.insert(item.id, jewels);
        }
    });
    GRID.with(|g| {
        if let Some(r) = g
            .as_mut()
            .and_then(|g| g.rows.iter_mut().find(|r| r.instance.id == item.id))
        {
            r.record[2] = u32::from(jewels[0]) << 16;
            r.record[3] = u32::from(jewels[1]) | u32::from(jewels[2]) << 16;
        }
    });
    if inventory::selected().is_some_and(|(w, _)| w.id == item.id) {
        decorations::refresh();
        crate::weapon::profile::selection_changed();
    }
}

#[forge::hook(offset = 0x14526c)]
unsafe fn insert(save: u32, slot: u32, index: u32, jewel: u32) -> u32 {
    if for_save(save).is_none() {
        return original!(save, slot, index, jewel);
    }
    if index < 3
        && fits(slot, Some(index as usize), jewel)
        && let Some(item) = item(slot)
    {
        let mut jewels = decorations::installed(item.id);
        jewels[index as usize] = jewel as u16;
        write(item, jewels);
    }
    0
}

#[forge::hook(offset = 0x1454a8)]
unsafe fn remove(save: u32, slot: u32, index: u32, compact: u32) -> u32 {
    if for_save(save).is_none() {
        return original!(save, slot, index, compact);
    }
    let Some(item) = item(slot) else { return 0 };
    let mut jewels = decorations::installed(item.id);
    let Some(target) = jewels.get_mut(index as usize) else {
        return 0;
    };
    let removed = core::mem::take(target);
    if compact == 1 {
        jewels[index as usize..].rotate_left(1);
    }
    write(item, jewels);
    u32::from(removed)
}

#[forge::hook(offset = 0x1456bc)]
unsafe fn clear_all(save: u32) -> u32 {
    if for_save(save).is_none() {
        return original!(save);
    }
    // 19A8D8 already transferred the visible inventory's jewels to the box.
    for i in 0..INVENTORY_LIMIT as u32 {
        if let Some(item) = item(i)
            && decorations::installed(item.id) != [0; 3]
        {
            write(item, [0; 3]);
        }
    }
    0
}

#[forge::hook(offset = 0x72c7e0)]
unsafe fn socket_panel(owner: u32, record: u32, page: u32) -> u32 {
    if let Some(w) = unsafe { projected(record) } {
        unsafe {
            set(owner, 890, w.level().slots);
            set(owner, 892, u16::from(w.definition.equipment.rarity) - 1);
            set(owner, 894, u16::from(w.definition.kind.icon));
            set(
                owner,
                896,
                crate::weapon::icon::equipment_color(w.definition),
            );
            set(owner, 900, w.definition.name.as_ptr() as u32);
            gu!(0xdadf0, owner + 904, record);
        }
    }
    original!(owner, record, page)
}

#[forge::hook(offset = 0x72cec4)]
unsafe fn preview(owner: u32, record: u32, other: u32, flags: u32, widget: u32) -> u32 {
    if let Some(w) = unsafe { projected(record) } {
        unsafe {
            let panel = gu!(0x65cbe8, owner, widget);
            crate::weapon::menu::details(
                panel,
                Some(w.definition),
                u32::from(w.upgrade) + 1,
                w.jewels,
            );
        }
        return 0;
    }
    original!(owner, record, other, flags, widget)
}

#[forge::hook(offset = 0x26e964)]
unsafe fn preview_part(dest: u32, part: u32, slot: u32, flags: u32) -> u32 {
    if let Some(w) = unsafe { projected(part) } {
        records::remember(dest, Some(w));
        return dest;
    }
    original!(dest, part, slot, flags)
}

#[forge::hook(offset = 0x5b90a8)]
unsafe fn sale_label(panel: u32, name: u32, price: u32, color: u32) -> u32 {
    if let Some(owner) = panel_owner(panel)
        && scope().is_some_and(|s| s.owner == owner)
        && let Some((item, def)) = row(unsafe { read(owner, 824) }, |r| (r.instance, r.equipment))
    {
        return original!(
            panel,
            def.name.as_ptr() as u32,
            trade::sale_price(item).unwrap_or(u32::MAX),
            color
        );
    }
    original!(panel, name, price, color)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, preview_part);
    forge::install_hook!(base, sale_label);
    forge::install_hook!(base, price);
    forge::install_hook!(base, sell);
    forge::install_hook!(base, eligibility);
    forge::install_hook!(base, set_index);
    forge::install_hook!(base, swap);
    forge::install_hook!(base, sockets);
    forge::install_hook!(base, fits_empty);
    forge::install_hook!(base, fits_replacing);
    forge::install_hook!(base, insert);
    forge::install_hook!(base, remove);
    forge::install_hook!(base, clear_all);
    forge::install_hook!(base, socket_panel);
    forge::install_hook!(base, preview);
}
