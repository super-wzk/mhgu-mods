//! GU 1.4 weapon commerce. New equipment exists only in the SDK inventory.

use super::inventory;
use crate::save::{CHARACTER, character_loaded};
use alloc::{collections::BTreeMap, vec::Vec};
use mhgu_sdk::{
    sync::Shared,
    weapon::trade::{self, Material, Recipe},
};

unsafe fn read<T: Copy>(address: u32) -> T {
    unsafe { (address as *const T).read_unaligned() }
}

macro_rules! gu {
    ($off:expr, $($arg:expr),*) => {{
        let f: unsafe extern "C" fn($(gu!(@word $arg)),*) -> u32 =
            core::mem::transmute(forge::mem::text_addr() + $off);
        f($($arg),*)
    }};
    (@word $arg:expr) => {u32};
}

mod menu;

fn global(offset: u32) -> u32 {
    unsafe { read(forge::mem::text_addr() + offset) }
}

#[derive(Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
enum Kind {
    Purchase,
    Forge,
    Upgrade(u16),
}

static RECIPES: Shared<BTreeMap<(u16, Kind), Recipe>> = Shared::new(BTreeMap::new());

fn register(equipment: u16, kind: Kind, recipe: Recipe) -> u32 {
    let Some((_, definition)) = inventory::definition(equipment) else {
        return 1;
    };
    if recipe.zenny > 9_999_999
        || matches!(kind, Kind::Upgrade(level) if level >= definition.max_upgrade())
        || recipe.materials.iter().enumerate().any(|(i, m)| {
            if m.item == 0 {
                m.quantity != 0
            } else {
                m.quantity == 0
                    || m.quantity > 255
                    || recipe.materials[..i]
                        .iter()
                        .any(|other| other.item == m.item)
            }
        })
    {
        return 1;
    }
    RECIPES.with(|recipes| match recipes.entry((equipment, kind)) {
        alloc::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(recipe);
            0
        }
        alloc::collections::btree_map::Entry::Occupied(_) => 2,
    })
}

extern "C" fn purchase(equipment: u16, zenny: u32) -> u32 {
    register(
        equipment,
        Kind::Purchase,
        Recipe {
            zenny,
            materials: [Material::EMPTY; 4],
        },
    )
}

extern "C" fn forge(equipment: u16, recipe: Recipe) -> u32 {
    register(equipment, Kind::Forge, recipe)
}

extern "C" fn upgrade(equipment: u16, level: u16, recipe: Recipe) -> u32 {
    register(equipment, Kind::Upgrade(level), recipe)
}

#[derive(Clone, Copy)]
struct Offer {
    equipment: u16,
    kind: Kind,
    instance: Option<u32>,
    recipe: Recipe,
}

fn has_offers(smith: bool, weapon: u16) -> bool {
    let kind = if smith { Kind::Forge } else { Kind::Purchase };
    let matches_weapon = |equipment| {
        inventory::definition(equipment)
            .is_some_and(|(_, entry)| entry.equipment.weapon.get() == weapon)
    };
    RECIPES.with(|recipes| {
        recipes
            .keys()
            .any(|&(equipment, action)| action == kind && matches_weapon(equipment))
    })
}

fn offers(smith: bool) -> Vec<Offer> {
    let kind = if smith { Kind::Forge } else { Kind::Purchase };
    RECIPES.with(|recipes| {
        recipes
            .iter()
            .filter_map(|(&(equipment, action), &recipe)| {
                if action != kind {
                    return None;
                }
                inventory::definition(equipment)?;
                Some(Offer {
                    equipment,
                    kind,
                    instance: None,
                    recipe,
                })
            })
            .collect()
    })
}

fn upgrade_offer(item: mhgu_sdk::weapon::WeaponInstance) -> Option<Offer> {
    let equipment = item.equipment.get();
    let kind = Kind::Upgrade(item.upgrade);
    let recipe = RECIPES.with(|recipes| recipes.get(&(equipment, kind)).copied())?;
    Some(Offer { equipment, kind, instance: Some(item.id), recipe })
}

#[derive(Clone, Copy, PartialEq)]
enum SalePrice { Fixed(u32), Native(u32) }
static SALE_PRICES: Shared<BTreeMap<(u16, u16), SalePrice>> = Shared::new(BTreeMap::new());

extern "C" fn register_sale(id: u16, level: u16, zenny: u32) -> u32 {
    if zenny > 9_999_999 { return 1; }
    register_price(id, level, SalePrice::Fixed(zenny))
}

extern "C" fn register_native_sale(id: u16, level: u16, header: u32) -> u32 {
    if !(7..=21).contains(&(header & 31)) || header >> 16 == 0 || header & 0xfc00 != 0 { return 1; }
    register_price(id, level, SalePrice::Native(header))
}

fn register_price(id: u16, level: u16, price: SalePrice) -> u32 {
    if !inventory::definition(id).is_some_and(|(_, d)| level <= d.max_upgrade()) {
        return 1;
    }
    SALE_PRICES.with(|rows| match rows.entry((id, level)) {
        alloc::collections::btree_map::Entry::Vacant(entry) => { entry.insert(price); 0 }
        alloc::collections::btree_map::Entry::Occupied(entry) => if *entry.get() == price { 0 } else { 2 }
    })
}

pub(super) fn native_level_header(equipment: u16, upgrade: u16) -> Option<u32> {
    SALE_PRICES.with(|rows| match rows.get(&(equipment, upgrade)) {
        Some(SalePrice::Native(header)) => Some(*header),
        _ => None,
    })
}

pub(super) fn sale_price(item: mhgu_sdk::weapon::WeaponInstance) -> Option<u32> {
    let source = SALE_PRICES.with(|rows| rows.get(&(item.equipment.get(), item.upgrade)).copied())?;
    let mut price = match source {
        SalePrice::Fixed(price) => price,
        SalePrice::Native(header) => {
            let mut record = [0u32; 11];
            record[1] = header;
            let save = global(0x1856fe8);
            if save == 0 { return None; }
            unsafe { gu!(0x1434e4, save, record.as_ptr() as u32) }
        }
    };
    // GU 1434E4 includes the item sale price of each installed decoration.
    for jewel in super::decorations::installed(item.id).into_iter().filter(|&j| j != 0) {
        let record = item_record(jewel);
        if record == 0 { return None; }
        price = price.saturating_add(unsafe { read::<u32>(record + 28) });
    }
    Some(price.min(9_999_999))
}

pub(super) fn sell(item: mhgu_sdk::weapon::WeaponInstance, quote: u32) -> bool {
    let player = wallet();
    if player == 0 || !character_loaded() || inventory::owned_equipment(item.id).map(|r| r.0) != Some(item)
        || sale_price(item) != Some(quote) { return false; }
    let removed = CHARACTER.with(|p| {
        let bag = &mut p.inventory;
        if bag.equipped == Some(item.id) || bag.instances.remove(&item.id).is_none() { return false; }
        bag.decorations.remove(&item.id);
        bag.sets.retain(|slot, link| {
            if link.instance != item.id { return true; }
            p.equipment_sets.remove(slot);
            false
        });
        if let Some(slot) = bag.slots.iter_mut().find(|id| **id == item.id) { *slot = 0; }
        true
    });
    // The native sale controller credits its wallet exactly once.
    removed
}

fn wallet() -> u32 {
    global(0x1897f78)
}

fn balance() -> u32 {
    let player = wallet();
    if player == 0 {
        0
    } else {
        unsafe { gu!(0x523178, player) }
    }
}

pub(super) fn item_record(item: u16) -> u32 {
    let table = global(0x18583a8);
    if table == 0 || unsafe { read::<u32>(table + 28) } == 0 {
        0
    } else {
        unsafe { gu!(0x194228, table, u32::from(item)) }
    }
}

pub(super) fn available(item: u16) -> u32 {
    if global(0x18584b8) == 0 || global(0x1858438) == 0 {
        return 0;
    }
    unsafe { (gu!(0x54fe8c, u32::from(item)) as i32).max(0) as u32 }
}

fn materials(recipe: &Recipe) -> impl Iterator<Item = Material> + '_ {
    recipe.materials.iter().copied().filter(|m| m.item != 0)
}

#[derive(Clone, Copy)]
enum Blocked {
    Space,
    Money,
    Material,
    Deduction,
    Changed,
}

impl Offer {
    fn check(self, bag: &crate::save::Inventory) -> Result<(), Blocked> {
        if let Some(id) = self.instance {
            if !bag.instances.get(&id).is_some_and(|item| {
                item.equipment == self.equipment && self.kind == Kind::Upgrade(item.upgrade)
            }) {
                return Err(Blocked::Changed);
            }
        } else if inventory::next_id(bag).is_none() {
            return Err(Blocked::Space);
        }
        if wallet() == 0 || balance() < self.recipe.zenny {
            return Err(Blocked::Money);
        }
        if materials(&self.recipe)
            .any(|m| item_record(m.item) == 0 || available(m.item) < u32::from(m.quantity))
        {
            return Err(Blocked::Material);
        }
        Ok(())
    }

    fn complete(self, spend: impl FnOnce()) -> Result<u32, Blocked> {
        if !character_loaded() {
            return Err(Blocked::Changed);
        }
        let id = CHARACTER.with(|data| {
            let bag = &mut data.inventory;
            self.check(bag)?;
            if !spend_checked(&self.recipe, spend) {
                return Err(Blocked::Deduction);
            }
            if let Some(id) = self.instance {
                bag.instances.get_mut(&id).unwrap().upgrade += 1;
                Ok(id)
            } else {
                Ok(inventory::insert(bag, self.equipment).unwrap())
            }
        })?;
        if self.instance.is_some() {
            inventory::notify_equipped_upgrade(id);
        }
        Ok(id)
    }
}

fn spend_checked(recipe: &Recipe, spend: impl FnOnce()) -> bool {
    let money = balance();
    if materials(recipe).next().is_none() {
        spend();
        let remaining = balance();
        if remaining == money - recipe.zenny {
            return true;
        }
        unsafe { gu!(0x523150, wallet(), money.wrapping_sub(remaining)) };
        return false;
    }
    // GU's crafting debit doesn't report partial material deductions. Keep
    // the original layout of the pouch and box until every debit is verified.
    let pouch = (global(0x18584b8) + 26) as *mut u8;
    let item_box = (global(0x1858438) + 26) as *mut u8;
    unsafe {
        let pouch_before = core::slice::from_raw_parts(pouch, 50 * 4).to_vec();
        let box_before = core::slice::from_raw_parts(item_box, 2300 * 4).to_vec();
        let counts = recipe.materials.map(|m| {
            if m.item == 0 {
                0
            } else {
                available(m.item) - u32::from(m.quantity)
            }
        });
        spend();
        if balance() == money - recipe.zenny
            && recipe
                .materials
                .iter()
                .zip(counts)
                .all(|(m, count)| m.item == 0 || available(m.item) == count)
        {
            return true;
        }
        core::ptr::copy_nonoverlapping(pouch_before.as_ptr(), pouch, pouch_before.len());
        core::ptr::copy_nonoverlapping(box_before.as_ptr(), item_box, box_before.len());
        gu!(0x523150, wallet(), money.wrapping_sub(balance()));
        false
    }
}

pub(super) fn install(base: u32) {
    static API: trade::Service = trade::Service {
        purchase,
        forge,
        upgrade,
    };
    static SALE: trade::SaleRegistration = trade::SaleRegistration { price: register_sale, native_row: register_native_sale };
    unsafe { trade::publish_sale(&SALE); }
    if unsafe { trade::publish(&API) } {
        menu::install(base);
    }
}
