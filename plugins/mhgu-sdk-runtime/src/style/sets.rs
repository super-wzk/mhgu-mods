//! GU's hunter My Set mutations. The native record still owns equipment and arts.
use super::{Entry, PREFERENCES, active, choose, local_hunter, registered_entry};
use crate::save::{SETS, SetStyle, WEAPONS, custom_id};
use mhgu_sdk::style::hunter;

unsafe fn record(save: u32, slot: u32) -> Option<u32> {
    if save == 0 || slot >= SETS {
        return None;
    }
    // GU 146408: 40 records of 136 bytes starting at save manager +254028.
    Some(save + 254028 + 136 * slot)
}

unsafe fn fingerprint(record: u32) -> u32 {
    // GU 14709C copies the semantic fields while swapping sets, excluding
    // the name (0..42) and padding before the talisman data and at the end.
    let equipment = unsafe { core::slice::from_raw_parts((record + 42) as *const u8, 56) };
    let bonuses = unsafe { core::slice::from_raw_parts((record + 100) as *const u8, 35) };
    equipment
        .iter()
        .chain(bonuses)
        .fold(0x811c9dc5, |hash, &byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
        })
}

pub(super) unsafe fn preview(slot: u32, weapon: u32) -> Option<&'static Entry> {
    let save = unsafe { *((forge::mem::text_addr() + 0x1856fe8) as *const u32) };
    unsafe { saved_entry(save, slot, weapon) }
}

unsafe fn saved_entry(save: u32, slot: u32, weapon: u32) -> Option<&'static Entry> {
    let record = unsafe { record(save, slot) }?;
    let saved = PREFERENCES.with(|p| p.equipment_sets.get(&(slot as u8)).copied())?;
    if u32::from(saved.weapon) != weapon || saved.fingerprint != unsafe { fingerprint(record) } {
        return None;
    }
    registered_entry(u32::from(saved.id)).filter(|entry| entry.style.weapon == weapon)
}

#[forge::hook(offset = 0x146cb8)]
unsafe fn store_set(save: u32, slot: u32) -> u32 {
    let out = original!(save, slot);
    if out == 1
        && let Some(record) = unsafe { record(save, slot) }
    {
        let actor = unsafe { local_hunter() };
        let style = (actor != 0)
            .then(|| unsafe { hunter::equipped_weapon(actor) })
            .flatten()
            .and_then(|weapon| {
                Some(SetStyle {
                    weapon,
                    id: custom_id(active(u32::from(weapon))?.style.id)?,
                    fingerprint: unsafe { fingerprint(record) },
                })
            });
        PREFERENCES.with(|p| {
            if let Some(style) = style {
                p.equipment_sets.insert(slot as u8, style);
            } else {
                p.equipment_sets.remove(&(slot as u8));
            }
        });
    }
    out
}

#[forge::hook(offset = 0x147250)]
unsafe fn delete_set(save: u32, slot: u32) -> u32 {
    let out = original!(save, slot);
    if slot < SETS {
        PREFERENCES.with(|p| p.equipment_sets.remove(&(slot as u8)));
    }
    out
}

#[forge::hook(offset = 0x1480b4)]
unsafe fn swap_sets(save: u32, prowler: u32, first: u32, second: u32) -> u32 {
    let out = original!(save, prowler, first, second);
    if prowler == 0 && first < SETS && second < SETS && first != second {
        PREFERENCES.with(|p| {
            let first = first as u8;
            let second = second as u8;
            let a = p.equipment_sets.remove(&first);
            let b = p.equipment_sets.remove(&second);
            if let Some(style) = a {
                p.equipment_sets.insert(second, style);
            }
            if let Some(style) = b {
                p.equipment_sets.insert(first, style);
            }
        });
    }
    out
}

#[forge::hook(offset = 0x146560)]
unsafe fn apply_set(save: u32, slot: u32) -> u32 {
    let Some(record) = (unsafe { record(save, slot) }) else {
        return original!(save, slot);
    };
    let base = forge::mem::text_addr();
    let equipped_index: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(base + 0xe100c) };
    // A set with no weapon must not change the preference for the old weapon.
    let expected = unsafe { *((record + 42) as *const u16) };
    // GU's My Set uses 2772C4, which resolves the same local actor as the SDK.
    let can_apply = expected != u16::MAX && unsafe { local_hunter() } != 0;
    let out = original!(save, slot);
    if can_apply {
        let actor = unsafe { local_hunter() };
        if actor != 0 {
            let equipment = unsafe { *((actor + 5060) as *const u32) };
            if equipment != 0 && unsafe { equipped_index(equipment + 4, 0) } == u32::from(expected)
            {
                let weapon = unsafe { crate::style::profile::equipment_weapon(equipment) };
                if (weapon as usize) < WEAPONS {
                    choose(weapon, unsafe { saved_entry(save, slot, weapon) });
                }
            }
        }
    }
    out
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, store_set);
    forge::install_hook!(base, delete_set);
    forge::install_hook!(base, swap_sets);
    forge::install_hook!(base, apply_set);
}
