//! Project SDK-owned instances into GU's phase 7 equipment upgrade controller.
use super::super::{Blocked, balance, upgrade_offer};
use super::{global, inventory, native, put, read};
use crate::save::CHARACTER;
use crate::weapon::{
    icon,
    menu::{details as display, grid},
    model,
};
use mhgu_sdk::weapon::WeaponInstance;

pub(super) use grid::{smith_active as active, smith_open as opened};

pub(super) unsafe fn open(owner: u32, original: impl FnOnce() -> u32) -> u32 {
    unsafe {
        // The previous forge page may have belonged to an SDK category.
        // GU rebuilds that retail page on leaving the upgrade controller.
        put(owner + 98, 0u16);
        grid::open_smith(owner);
        tick(owner, original)
    }
}

pub(super) unsafe fn tick(owner: u32, original: impl FnOnce() -> u32) -> u32 {
    let result = grid::with_smith(owner, original);
    unsafe {
        if read::<u8>(owner + 4) != 7 {
            grid::close();
            model::preview(0);
            if read::<u8>(owner + 4) == 1 {
                gu!(0x557d90, owner + 8036, 2);
                super::smith_home::extend(owner + 8036);
                gu!(0x557d90, owner + 8036, 3);
            }
        }
    }
    result
}

unsafe fn selection(owner: u32) -> Option<WeaponInstance> {
    unsafe {
        let width = u32::from(read::<u8>(owner + 8382));
        let height = u32::from(read::<u8>(owner + 8383));
        let page = u32::from(read::<u16>(owner + 8384));
        let x = u32::from(read::<u8>(owner + 8380));
        let y = u32::from(read::<u8>(owner + 8381));
        grid::smith_instance(x + width * (y + height * page))
    }
}

unsafe fn show(owner: u32, panel: u32, item: Option<WeaponInstance>, next: bool) {
    let entry = item.and_then(|item| inventory::definition(item.equipment.get()).map(|(_, e)| e));
    let level = item.map_or(0, |item| 1 + u32::from(item.upgrade) + u32::from(next));
    unsafe {
        let view = gu!(0x65cb3c, owner, panel);
        if view == 0 {
            return;
        }
        let comparing = read::<u8>(owner + 57) != 0;
        // GU 6F9B18 labels the main panel for the current comparison mode
        // and uses separate text colours on the candidate panel.
        if panel == 5 {
            put(view + 84, if comparing { 11u32 } else { 9 });
        }
        let colours = if panel == 6 && comparing {
            0x18a4180
        } else {
            0x191ea90
        };
        put(view + 176, global(colours));
        put(view + 180, global(colours + 4));
        let description = read::<u8>(owner + 8085) != 0;
        put(view + 248, if description { 5u32 } else { 1 });
        let jewels = item.map_or([0; 3], |i| crate::weapon::decorations::installed(i.id));
        display(view, entry, level, jewels);
    }
}

#[forge::hook(offset = 0x6ff02c)]
unsafe fn box_details(
    owner: u32,
    page: u32,
    slot: u32,
    init: u32,
    mode: u32,
    compare: u32,
    other_page: u32,
    other_slot: u32,
) -> u32 {
    if !active(owner) {
        return original!(
            owner, page, slot, init, mode, compare, other_page, other_slot
        );
    }
    let item = grid::smith_instance(page * 100 + slot);
    let enabled = item.and_then(upgrade_offer).is_some();
    unsafe {
        put(owner + 11808, if enabled { 0u32 } else { 0x400 });
        if init != 0 {
            gu!(0x54edec, owner + 8072, 2, 2, 0);
        }
        show(owner, 5, item, false);
    }
    0
}

pub(super) unsafe fn details(owner: u32) -> u32 {
    unsafe {
        let item = selection(owner);
        let comparing = read::<u8>(owner + 57) != 0;
        let next = item.and_then(upgrade_offer).is_some();
        show(owner, 5, item, !comparing && next);
        if comparing {
            show(owner, 6, item, next);
        }
    }
    0
}

pub(super) unsafe fn preview(owner: u32) -> u32 {
    let Some(item) = (unsafe { selection(owner) }) else {
        return 0;
    };
    let viewer = unsafe { read::<u32>(owner + 12816) };
    if viewer != 0 {
        let (category, id) = native::preview_pose();
        model::preview(item.equipment.get());
        unsafe {
            put(viewer + 72, u32::from(category));
            put(viewer + 76, u32::from(id));
            put(viewer + 53, 0u8);
            put(viewer + 55, 0u8);
            put(viewer + 84, 0u32);
            if read::<u8>(owner + 59) == 0 {
                put(owner + 59, 1u8);
                put(viewer + 60, 1u32);
                gu!(0x65cba0, owner, 24);
            }
        }
    }
    0
}

#[forge::hook(offset = 0x6ffd40)]
unsafe fn recipes(owner: u32, page: u32, init: u32) -> u32 {
    if !active(owner) {
        return original!(owner, page, init);
    }
    unsafe {
        let item = selection(owner);
        let offer = item.and_then(upgrade_offer);
        let record = owner + 10452;
        let row = owner + 11616;
        core::ptr::write_bytes(record as *mut u8, 0, 48 * 8);
        core::ptr::write_bytes(row as *mut u8, 0, 24 * 8);
        put(owner + 100, 1u16);
        put(record, 0x200u16);
        put(row + 6, 2u8);
        if let Some(item) = item
            && let Some((_, entry)) = inventory::definition(item.equipment.get())
        {
            put(row, icon::equipment_color(entry));
            put(row + 4, u16::from(entry.kind.icon));
            put(row + 20, entry.name.as_ptr() as u32);
            let messages = global(0x1899220);
            put(
                row + 16,
                gu!(
                    0x55bc7c,
                    messages,
                    14,
                    if offer.is_some() { 51 } else { 197 }
                ),
            );
        }
        if let Some(offer) = offer {
            let status = CHARACTER.with(|d| match offer.check(&d.inventory) {
                Ok(()) => 0u16,
                Err(Blocked::Money) => 4,
                Err(Blocked::Material) => 8,
                Err(_) => 0x200,
            });
            put(record, status);
            put(record + 2, offer.equipment);
            put(record + 12, 1u16);
            put(record + 16, offer.recipe.zenny);
            put(row + 8, offer.recipe.zenny);
            put(row + 6, if status == 0 { 1u8 } else { 2 });
            for (i, mat) in offer.recipe.materials.iter().enumerate() {
                put(record + 20 + i as u32 * 4, mat.item);
                put(record + 22 + i as u32 * 4, mat.quantity as u8);
            }
        }
        if init != 0 {
            gu!(0x54edec, owner + 8036, 8, 1, 0);
            gu!(0x54ee34, owner + 8036, 1, 3);
            gu!(0x54edec, owner + 8072, 2, 2, 0);
            gu!(0x54ecc0, owner + 8072, 0);
            gu!(0x54edec, owner + 8108, 10, 1, 0);
            gu!(0x54edec, owner + 8144, 1, 3, 0);
        }
        details(owner)
    }
}

// GU normally writes a retail equipment record at 70301C. Use its same
// 6FBCFC debit and apply the level only to the selected SDK instance.
#[forge::hook(offset = 0x702d9c)]
unsafe fn finish(owner: u32) -> u32 {
    if !active(owner) {
        return original!(owner);
    }
    unsafe {
        let offer = selection(owner).and_then(upgrade_offer);
        if let Some(id) = offer.and_then(|o| {
            o.complete(|| {
                gu!(0x6fbcfc, owner);
            })
            .ok()
        }) {
            if let Some((item, _)) = inventory::owned_equipment(id) {
                grid::upgraded(item);
            }
            gu!(0x54f260, 1, 118);
            gu!(0x65cbc4, owner, 13);
            let funds = gu!(0x65cb3c, owner, 3);
            if funds != 0 {
                gu!(0x5d0d44, funds, balance());
            }
            if inventory::selected().is_some_and(|(item, _)| item.id == id) {
                put(owner + 6, 12u8);
            } else {
                gu!(0x70361c, owner);
                put(owner + 57, 0u8);
                gu!(0x65cbc4, owner, 6);
                gu!(0x65cbc4, owner, 2);
            }
        } else {
            gu!(0x703990, owner);
            gu!(0x54f260, 0, 3);
        }
    }
    0
}

// Selecting an SDK instance uses the same yes/no widget and messages as
// 7045DC state 10, without passing an expansion slot to the retail save.
#[forge::hook(offset = 0x7045dc)]
unsafe fn upgrade_input(owner: u32) -> u32 {
    if !active(owner) || unsafe { read::<u8>(owner + 6) } != 10 {
        return original!(owner);
    }
    unsafe {
        let cursor = owner + 67;
        let previous = read::<u8>(cursor);
        match gu!(0x54fd94, cursor, read::<u32>(owner + 108), 128, 32) {
            1 => {
                if let Some(item) = selection(owner) {
                    inventory::select(Some(item.id));
                }
                put(owner + 6, 12u8);
                gu!(0x54f260, 1, 101);
            }
            2 => {
                put(owner + 6, 12u8);
                gu!(0x54f260, 0, 31);
            }
            _ if read::<u8>(cursor) != previous => {
                gu!(0x65e5bc, 20, 1, 14, 69, u32::from(read::<u8>(cursor)), 0, 0);
            }
            _ => {}
        }
    }
    0
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, box_details);
    forge::install_hook!(base, recipes);
    forge::install_hook!(base, finish);
    forge::install_hook!(base, upgrade_input);
}
