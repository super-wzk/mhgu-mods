//! Identity of registered weapons in native equipment aggregates and temporary records.
use super::{gu, inventory, profile, read};
use mhgu_sdk::sync::Shared;

#[derive(Clone, Copy)]
pub(super) struct DisplayEquipment {
    pub definition: inventory::RegisteredEquipment,
    pub upgrade: u16,
    pub jewels: super::decorations::Jewels,
}

impl DisplayEquipment {
    pub fn level(self) -> mhgu_sdk::weapon::EquipmentLevel {
        self.definition.levels[usize::from(self.upgrade)]
    }

    pub fn owned(item: mhgu_sdk::weapon::WeaponInstance) -> Option<Self> {
        let mut display = Self::registered(item.equipment.get(), item.upgrade)?;
        display.jewels = super::decorations::installed(item.id);
        Some(display)
    }
    pub fn registered(id: u16, upgrade: u16) -> Option<Self> {
        let definition = inventory::definition(id)?.1;
        definition.levels.get(usize::from(upgrade))?;
        Some(Self {
            definition,
            upgrade,
            jewels: [0; 3],
        })
    }
}

// GU copies the seven-part aggregate with E175C. Keep provenance on the
// weapon record alone so replacing armour retains it and replacing the weapon drops it.
static COPIES: Shared<alloc::collections::BTreeMap<u32, DisplayEquipment>> =
    Shared::new(alloc::collections::BTreeMap::new());

fn copied(equipment: u32) -> Option<DisplayEquipment> {
    COPIES.with(|rows| rows.get(&(equipment + 24)).copied())
}

pub(super) fn remember(equipment: u32, weapon: Option<DisplayEquipment>) {
    assign(equipment + 24, weapon);
}

fn assign(record: u32, weapon: Option<DisplayEquipment>) -> Option<DisplayEquipment> {
    COPIES.with(|rows| {
        if let Some(weapon) = weapon {
            rows.insert(record, weapon)
        } else {
            rows.remove(&record)
        }
    })
}

#[forge::hook(offset = 0x3e1208)]
unsafe fn read_slot(manager: u32, slot: u32, dest: u32) -> u32 {
    let result = original!(manager, slot, dest);
    if dest != 0 {
        let weapon = (result != 0 && slot < 3)
            .then(|| crate::save::preview_weapon(slot as u8))
            .flatten()
            .and_then(|(item, jewels)| {
                let mut w = DisplayEquipment::registered(item.equipment, item.upgrade)?;
                w.jewels = jewels;
                Some(w)
            });
        remember(dest, weapon);
    }
    result
}

#[forge::hook(offset = 0xe175c)]
unsafe fn copy_stats(dest: u32, src: u32) -> u32 {
    let target = dest - 4;
    let weapon = unsafe { display(src - 4) };
    let manager: u32 = unsafe { read(forge::mem::text_addr(), 0x1885b58) };
    let refresh = manager != 0
        && target == manager + 12508
        && copied(target).map(|w| w.definition.equipment.id)
            != weapon.map(|w| w.definition.equipment.id);
    let result = original!(dest, src);
    remember(target, weapon);
    if refresh {
        unsafe {
            let actor: u32 = read(manager, 44);
            if actor != 0 && inventory::actor_equipment(actor) == target {
                gu!(0x2861c4, actor, target + 24);
            }
        }
    }
    result
}

#[forge::hook(offset = 0xdadf0)]
unsafe fn replace_part(dest: u32, src: u32) -> u32 {
    if dest != src {
        assign(dest, None);
    }
    original!(dest, src)
}

#[forge::hook(offset = 0xe15f4)]
unsafe fn clear_stats(stats: u32) -> u32 {
    remember(stats - 4, None);
    original!(stats)
}

#[forge::hook(offset = 0xe14c8)]
unsafe fn destroy_stats(stats: u32) -> u32 {
    remember(stats - 4, None);
    original!(stats)
}

pub(super) fn with_record<T>(record: u32, weapon: DisplayEquipment, f: impl FnOnce() -> T) -> T {
    let previous = assign(record, Some(weapon));
    let result = f();
    assign(record, previous);
    result
}

pub(super) unsafe fn for_record(record: u32) -> Option<DisplayEquipment> {
    unsafe { super::menu::grid::projected(record).or_else(|| display(record.checked_sub(24)?)) }
}

pub(super) unsafe fn display(equipment: u32) -> Option<DisplayEquipment> {
    if equipment == 0 {
        return None;
    }
    if unsafe { inventory::is_native_local(equipment) } {
        return DisplayEquipment::owned(inventory::selected()?.0);
    }
    copied(equipment).or_else(|| unsafe { profile::room(equipment)?.1 })
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, read_slot);
    forge::install_hook!(base, copy_stats);
    forge::install_hook!(base, replace_part);
    forge::install_hook!(base, clear_stats);
    forge::install_hook!(base, destroy_stats);
}
