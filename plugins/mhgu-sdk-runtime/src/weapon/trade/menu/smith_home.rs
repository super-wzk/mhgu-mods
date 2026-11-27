//! Extra smithy home row between retail equipment upgrades and decorations.
use super::super::{Kind, RECIPES};
use super::{ACCEPT, BACK, Place, global, put, read, upgrade};
use crate::save::{CHARACTER, character_loaded};

const ROW: u8 = 3;
const TITLE: &core::ffi::CStr = c"扩展装备强化";

fn availability() -> Option<bool> {
    character_loaded()
        .then(|| {
            CHARACTER.with(|data| {
                RECIPES.with(|recipes| {
                    recipes
                        .keys()
                        .any(|(_, kind)| matches!(kind, Kind::Upgrade(_)))
                        .then(|| {
                            data.inventory.instances.values().any(|i| {
                                recipes.contains_key(&(i.equipment, Kind::Upgrade(i.upgrade)))
                            })
                        })
                })
            })
        })
        .flatten()
}

pub(super) unsafe fn extend(cursor: u32) {
    unsafe {
        if read::<u8>(cursor + 14) == 4 {
            let row = read::<u8>(cursor + 13);
            put(cursor + 14, 5u8);
            put(cursor + 20, 5u16);
            put(cursor + 13, row + u8::from(row >= ROW));
        }
    }
}

pub(super) unsafe fn draw(owner: u32, panel: u32, original: impl FnOnce() -> u32) -> u32 {
    let Some(enabled) = availability() else {
        return original();
    };
    unsafe {
        let cursor = owner + Place::Smith.cursor();
        extend(cursor);
        let desc = owner + Place::Smith.category_rows();
        let mut rows = [0u32; 11];
        rows[..9].copy_from_slice(&read::<[u32; 9]>(desc));
        rows.copy_within(7..9, 9);
        rows[7] = if enabled { 1 } else { 2 };
        rows[8] = TITLE.as_ptr() as u32;
        put(panel + 84, rows.as_ptr() as u32);
        let result = original();
        put(panel + 84, desc);
        result
    }
}

pub(super) unsafe fn update(owner: u32, original: impl FnOnce() -> u32) -> Option<u32> {
    let enabled = availability()?;
    unsafe {
        let cursor = owner + Place::Smith.cursor();
        extend(cursor);
        let pad = global(0x188c670);
        if pad == 0 || gu!(0x65d9e8,) != 0 {
            return None;
        }
        let row = read::<u8>(cursor + 13);
        let buttons = read::<u32>(pad + 27644);
        if buttons & (ACCEPT | BACK) == ACCEPT && read::<f32>(owner + 168) <= 0. {
            if row == ROW {
                if !enabled {
                    return Some(gu!(0x54f260, 0, 3));
                }
                put(cursor + 13, ROW - 1);
                return Some(upgrade::open(owner, original));
            }
            // The native dispatcher expects decorations at index 3.
            if row == ROW + 1 {
                put(cursor + 13, ROW)
            }
        }
        let result = original();
        if read::<u8>(owner + 4) == 1 {
            let next = read::<u8>(cursor + 13);
            if next != row && next >= ROW {
                // GU's native help strings for upgrading and decorations.
                gu!(
                    0x65e5bc,
                    4,
                    1,
                    14,
                    if next == ROW { 80 } else { 112 },
                    0,
                    0,
                    0
                );
            }
        }
        Some(result)
    }
}
