//! GU's hunter My Set mutations. The native record still owns equipment and arts.
use super::{
    inventory, local_hunter, profile,
    style::{Entry, active, choose_style, registered_entry},
};
use crate::save::{CHARACTER, SETS, SetStyle, SetWeapon, WEAPONS, custom_id};

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

pub(super) unsafe fn preview(slot: u32, equipment: u32) -> Option<&'static Entry> {
    if equipment == 0 {
        return None;
    }
    let save = unsafe { *((forge::mem::text_addr() + 0x1856fe8) as *const u32) };
    let record = unsafe { record(save, slot) }?;
    let fingerprint = unsafe { fingerprint(record) };
    let weapon = unsafe { profile::equipment_weapon(equipment) };
    saved_weapon(slot, fingerprint)
        .map(|(_, entry)| entry)
        .or_else(|| saved_style(slot, fingerprint, weapon))
}

pub(super) unsafe fn equipment_display(slot: u32) -> Option<profile::DisplayEquipment> {
    let save = unsafe { *((forge::mem::text_addr() + 0x1856fe8) as *const u32) };
    let record = unsafe { record(save, slot) }?;
    let (item, _) = saved_weapon(slot, unsafe { fingerprint(record) })?;
    profile::DisplayEquipment::registered(item.equipment.get(), item.upgrade)
}

fn saved_style(slot: u32, fingerprint: u32, weapon: u32) -> Option<&'static Entry> {
    let saved = CHARACTER.with(|p| p.equipment_sets.get(&(slot as u8)).copied())?;
    if u32::from(saved.weapon) != weapon || saved.fingerprint != fingerprint {
        return None;
    }
    registered_entry(u32::from(saved.id))
        .filter(|entry| entry.style.weapon == weapon && entry.kind.is_none())
}

fn saved_weapon(
    slot: u32,
    fingerprint: u32,
) -> Option<(mhgu_sdk::weapon::WeaponInstance, &'static Entry)> {
    let saved = CHARACTER.with(|p| p.inventory.sets.get(&(slot as u8)).copied())?;
    (saved.fingerprint == fingerprint)
        .then(|| inventory::owned_equipment(saved.instance))
        .flatten()
}

#[forge::hook(offset = 0x146cb8)]
unsafe fn store_set(save: u32, slot: u32) -> u32 {
    let actor = unsafe { local_hunter() };
    let out = original!(save, slot);
    if out == 1
        && let Some(record) = unsafe { record(save, slot) }
    {
        let equipment = unsafe { inventory::actor_equipment(actor) };
        let fingerprint = unsafe { fingerprint(record) };
        let extension = inventory::selected().map(|(item, _)| SetWeapon {
            instance: item.id,
            fingerprint,
        });
        let style = (equipment != 0)
            .then(|| active(unsafe { profile::equipment_weapon(equipment) }))
            .flatten()
            .and_then(|entry| {
                Some(SetStyle {
                    weapon: entry.style.weapon as u8,
                    id: custom_id(entry.style.id)?,
                    fingerprint,
                })
            });
        CHARACTER.with(|p| {
            store(&mut p.equipment_sets, slot as u8, style);
            store(&mut p.inventory.sets, slot as u8, extension);
        });
    }
    out
}

#[forge::hook(offset = 0x147250)]
unsafe fn delete_set(save: u32, slot: u32) -> u32 {
    let out = original!(save, slot);
    if slot < SETS {
        CHARACTER.with(|p| {
            p.equipment_sets.remove(&(slot as u8));
            p.inventory.sets.remove(&(slot as u8));
        });
    }
    out
}

#[forge::hook(offset = 0x1480b4)]
unsafe fn swap_sets(save: u32, prowler: u32, first: u32, second: u32) -> u32 {
    let out = original!(save, prowler, first, second);
    if prowler == 0 && first < SETS && second < SETS && first != second {
        CHARACTER.with(|p| {
            let first = first as u8;
            let second = second as u8;
            swap(&mut p.equipment_sets, first, second);
            swap(&mut p.inventory.sets, first, second);
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
                let weapon = unsafe { profile::equipment_weapon(equipment) };
                if (weapon as usize) < WEAPONS {
                    let fingerprint = unsafe { fingerprint(record) };
                    let instance = saved_weapon(slot, fingerprint).map(|(item, _)| item.id);
                    choose_style(weapon, saved_style(slot, fingerprint, weapon));
                    inventory::select(instance);
                }
            }
        }
    }
    out
}

fn store<T>(sets: &mut alloc::collections::BTreeMap<u8, T>, slot: u8, value: Option<T>) {
    if let Some(value) = value {
        sets.insert(slot, value);
    } else {
        sets.remove(&slot);
    }
}

fn swap<T>(sets: &mut alloc::collections::BTreeMap<u8, T>, first: u8, second: u8) {
    let a = sets.remove(&first);
    let b = sets.remove(&second);
    store(sets, first, b);
    store(sets, second, a);
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, store_set);
    forge::install_hook!(base, delete_set);
    forge::install_hook!(base, swap_sets);
    forge::install_hook!(base, apply_set);
}
