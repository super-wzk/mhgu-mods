//! SDK-owned equipment catalogue and per-character inventory.
use super::{
    local_hunter, profile,
    style::{Entry, register_entry, registered_entries},
};
use crate::save::{CHARACTER, INVENTORY_LIMIT, OwnedWeapon};
use alloc::collections::BTreeMap;
use core::ffi::{CStr, c_char};
use mhgu_sdk::sync::Shared;
use mhgu_sdk::weapon::{
    EquipmentLevel, Weapon, WeaponEquipment, WeaponInstance, registry, style::Style,
};

unsafe extern "C" fn register(
    weapon: *const Weapon,
    style: *const Style,
    weapon_name: *const c_char,
    name: *const c_char,
    details: *const c_char,
) -> u32 {
    let (Some(&weapon), Some(style)) = (unsafe { weapon.as_ref() }, unsafe { style.as_ref() })
    else {
        return 1;
    };
    if style.weapon != weapon.id.style_key()
        || weapon.id.get() == 0
        || name.is_null()
        || weapon_name.is_null()
    {
        return 1;
    }
    unsafe {
        register_entry(
            style,
            Some((weapon, CStr::from_ptr(weapon_name))),
            name,
            details,
        )
    }
}

pub(super) fn weapon_type(id: u16) -> Option<(&'static Entry, Weapon)> {
    registered_entries().find_map(|entry| {
        let kind = entry.kind?;
        (kind.id.get() == id).then_some((entry, kind))
    })
}

#[derive(Clone, Copy)]
pub(super) struct RegisteredEquipment {
    pub equipment: &'static WeaponEquipment,
    pub kind: Weapon,
    pub levels: &'static [EquipmentLevel],
    pub name: &'static CStr,
    pub details: &'static CStr,
    pub model: Option<mhgu_sdk::weapon::EquipmentModel>,
}

static EQUIPMENT: Shared<BTreeMap<u16, RegisteredEquipment>> = Shared::new(BTreeMap::new());

unsafe extern "C" fn register_equipment(
    raw: *const WeaponEquipment,
    levels: *const EquipmentLevel,
    count: u32,
    name: *const c_char,
    details: *const c_char,
    archive: *const c_char,
    mesh: *const c_char,
) -> u32 {
    let Some(equipment) = (unsafe { raw.as_ref() }) else {
        return 1;
    };
    let Some((_, kind)) = weapon_type(equipment.weapon.get()) else {
        return 1;
    };
    if equipment.id.get() == 0 || name.is_null() || details.is_null() {
        return 1;
    }
    if !(1..=255).contains(&count)
        || levels.is_null()
        || !(1..=11).contains(&equipment.rarity)
        || equipment.elements.iter().any(|e| e.id() > 9)
    {
        return 1;
    }
    let levels = unsafe { core::slice::from_raw_parts(levels, count as usize) };
    if levels.iter().any(|row| {
        row.attack < 0
            || row.sharpness_type >= 164
            || row.sharpness_level > 12
            || row.slots > 3
            || !(-100..=100).contains(&row.affinity)
            || row
                .element_values
                .iter()
                .zip(equipment.elements)
                .any(|(&v, kind)| v < 0 || kind.id() == 0 && v != 0)
    }) {
        return 1;
    }
    let model = match (archive.is_null(), mesh.is_null()) {
        (true, true) => None,
        (false, false) => {
            let archive = unsafe { CStr::from_ptr(archive) };
            let model = unsafe { CStr::from_ptr(mesh) };
            if [archive, model]
                .iter()
                .any(|path| !(1..64).contains(&path.to_bytes().len()))
            {
                return 1;
            }
            Some(mhgu_sdk::weapon::EquipmentModel { archive, model })
        }
        _ => return 1,
    };
    EQUIPMENT.with(|models| match models.entry(equipment.id.get()) {
        alloc::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(RegisteredEquipment {
                equipment,
                kind,
                levels,
                model,
                name: unsafe { CStr::from_ptr(name) },
                details: unsafe { CStr::from_ptr(details) },
            });
            0
        }
        _ => 2,
    })
}

pub(crate) fn is_model_archive(path: &CStr) -> bool {
    EQUIPMENT.with(|rows| {
        rows.values()
            .any(|r| r.model.is_some_and(|model| model.archive == path))
    })
}
impl RegisteredEquipment {
    pub fn max_upgrade(self) -> u16 {
        self.levels.len() as u16 - 1
    }
}
extern "C" fn base(id: u16) -> *const WeaponEquipment {
    EQUIPMENT.with(|m| m.get(&id).map_or(core::ptr::null(), |r| r.equipment))
}
extern "C" fn level(id: u16, upgrade: u16) -> *const EquipmentLevel {
    EQUIPMENT.with(|m| {
        m.get(&id)
            .and_then(|r| r.levels.get(usize::from(upgrade)))
            .map_or(core::ptr::null(), core::ptr::from_ref)
    })
}

extern "C" fn model_path(equipment: u16, archive: bool) -> *const c_char {
    EQUIPMENT
        .with(|m| m.get(&equipment).and_then(|record| record.model))
        .map_or(core::ptr::null(), |resource| {
            if archive {
                resource.archive.as_ptr()
            } else {
                resource.model.as_ptr()
            }
        })
}

pub(super) fn definition(id: u16) -> Option<(&'static Entry, RegisteredEquipment)> {
    let model = EQUIPMENT.with(|m| m.get(&id).copied())?;
    Some((weapon_type(model.equipment.weapon.get())?.0, model))
}

extern "C" fn grant(weapon: u16) -> u32 {
    if definition(weapon).is_none() || !crate::save::character_loaded() {
        return 0;
    }
    CHARACTER.with(|prefs| insert(&mut prefs.inventory, weapon).unwrap_or(0))
}

pub(super) fn next_id(inventory: &crate::save::Inventory) -> Option<u32> {
    if inventory.instances.len() >= INVENTORY_LIMIT {
        return None;
    }
    inventory
        .instances
        .last_key_value()
        .map_or(0, |(&id, _)| id)
        .checked_add(1)
}

pub(super) fn insert(inventory: &mut crate::save::Inventory, weapon: u16) -> Option<u32> {
    let id = next_id(inventory)?;
    inventory.instances.insert(
        id,
        OwnedWeapon {
            equipment: weapon,
            upgrade: 0,
        },
    );
    Some(id)
}

extern "C" fn upgrade(id: u32) -> bool {
    let upgraded = CHARACTER.with(|prefs| {
        let Some(item) = prefs.inventory.instances.get_mut(&id) else {
            return false;
        };
        let Some((_, kind)) = definition(item.equipment) else {
            return false;
        };
        if item.upgrade >= kind.max_upgrade() {
            return false;
        }
        item.upgrade += 1;
        true
    });
    if upgraded {
        notify_equipped_upgrade(id);
    }
    upgraded
}

pub(super) fn notify_equipped_upgrade(id: u32) {
    if CHARACTER.with(|prefs| prefs.inventory.equipped == Some(id)) {
        profile::selection_changed();
    }
}

fn owned(id: u32, item: OwnedWeapon) -> Option<(WeaponInstance, &'static Entry)> {
    let (entry, kind) = definition(item.equipment)?;
    let upgrade = item.upgrade.min(kind.max_upgrade());
    Some((
        WeaponInstance {
            id,
            equipment: kind.equipment.id,
            upgrade,
        },
        entry,
    ))
}

pub(super) fn owned_equipment(id: u32) -> Option<(WeaponInstance, &'static Entry)> {
    let item = CHARACTER.with(|prefs| prefs.inventory.instances.get(&id).copied())?;
    owned(id, item)
}

pub(super) fn inventory() -> alloc::vec::Vec<(WeaponInstance, &'static Entry)> {
    CHARACTER.with(|prefs| {
        prefs
            .inventory
            .instances
            .iter()
            .filter_map(|(&id, &item)| owned(id, item))
            .collect()
    })
}

/// Slots also reserve positions for equipment whose content plugin is currently absent.
pub(super) fn box_slots() -> alloc::vec::Vec<u32> {
    CHARACTER.with(|p| {
        let mut slots = p.inventory.slots.clone();
        for &id in p.inventory.instances.keys() {
            if !slots.contains(&id) {
                if let Some(empty) = slots.iter_mut().find(|slot| **slot == 0) {
                    *empty = id;
                } else {
                    slots.push(id);
                }
            }
        }
        slots
    })
}

pub(super) fn save_slots(mut slots: alloc::vec::Vec<u32>) {
    while slots.last() == Some(&0) {
        slots.pop();
    }
    CHARACTER.with(|p| p.inventory.slots = slots);
}

pub(super) fn select(id: Option<u32>) -> bool {
    if id.is_some_and(|id| owned_equipment(id).is_none()) {
        return false;
    }
    let changed =
        CHARACTER.with(|prefs| core::mem::replace(&mut prefs.inventory.equipped, id) != id);
    if changed {
        // 26E964 uses 2861C4 to schedule GU's live equipment-part refresh.
        unsafe {
            let actor = local_hunter();
            let eq = actor_equipment(actor);
            if eq != 0 && (7..=21).contains(&(read::<u16>(eq + 28) & 31)) {
                let dirty: unsafe extern "C" fn(u32, u32) =
                    core::mem::transmute(forge::mem::text_addr() + 0x2861c4);
                dirty(actor, eq + 24);
            }
            super::style::validate_arts(actor, eq);
            super::decorations::refresh();
        }
        profile::selection_changed();
    }
    true
}

unsafe fn read<T: Copy>(addr: u32) -> T {
    unsafe { (addr as *const T).read_unaligned() }
}

fn manager() -> u32 {
    unsafe { read(forge::mem::text_addr() + 0x1885b58) }
}

pub(super) unsafe fn current_equipment() -> u32 {
    unsafe {
        let manager = manager();
        if manager != 0 {
            let current: unsafe extern "C" fn(u32) -> u32 =
                core::mem::transmute(forge::mem::text_addr() + 0x275a30);
            return current(manager);
        }
        0
    }
}

pub(super) unsafe fn actor_equipment(actor: u32) -> u32 {
    if actor == 0 {
        0
    } else {
        unsafe { read(actor + 5060) }
    }
}

pub(super) unsafe fn local_equipment() -> u32 {
    let equipment = unsafe { actor_equipment(local_hunter()) };
    if equipment != 0 {
        equipment
    } else {
        unsafe { current_equipment() }
    }
}

fn native_hunter() -> bool {
    let manager = manager();
    // The manager selects separate equipment for Prowlers.
    manager != 0 && unsafe { read::<u8>(manager + 40028) } == 0
}

pub(super) unsafe fn is_native_local(equipment: u32) -> bool {
    native_hunter() && equipment == manager() + 124
}

pub(super) fn selected() -> Option<(WeaponInstance, &'static Entry)> {
    if !native_hunter() {
        return None;
    }
    owned_equipment(CHARACTER.with(|prefs| prefs.inventory.equipped)?)
}

extern "C" fn equipped() -> u32 {
    unsafe {
        let equipment = local_equipment();
        if equipment == 0 {
            return u32::MAX;
        }
        if let Some((_, entry)) = selected() {
            return entry.style.weapon;
        }
        profile::equipment_weapon(equipment)
    }
}

unsafe extern "C" fn instance(id: u32, out: *mut WeaponInstance) -> bool {
    let item = if id == 0 {
        selected().map(|(item, _)| item)
    } else {
        CHARACTER
            .with(|prefs| prefs.inventory.instances.get(&id).copied())
            .and_then(|item| owned(id, item))
            .map(|(item, _)| item)
    };
    if let (Some(item), Some(out)) = (item, unsafe { out.as_mut() }) {
        *out = item;
        true
    } else {
        false
    }
}

// The stock equipment-box confirmation (case 3) calls 144768 for equips.
#[forge::hook(offset = 0x144768)]
unsafe fn equip_native(save: u32, index: u32, refresh: u32) -> u32 {
    if let Some(result) = crate::weapon::menu::grid::equip(save, index) {
        return result;
    }
    if save != 0 && index < 2000 {
        let category = unsafe { read::<u8>(save + 44 * index + 32) } & 0x1f;
        if (7..=21).contains(&category) {
            select(None);
        }
    }
    original!(save, index, refresh)
}

pub(super) fn install(base: u32) {
    static API: registry::Service = registry::Service {
        register,
        register_equipment,
        model_path,
        register_icon: super::icon::register,
        base: self::base,
        level,
        equipped,
        grant,
        upgrade,
        instance,
    };
    unsafe { registry::publish(&API) };
    forge::install_hook!(base, equip_native);
}
