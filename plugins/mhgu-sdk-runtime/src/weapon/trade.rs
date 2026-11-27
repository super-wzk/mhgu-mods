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

fn item_record(item: u16) -> u32 {
    let table = global(0x18583a8);
    if table == 0 || unsafe { read::<u32>(table + 28) } == 0 {
        0
    } else {
        unsafe { gu!(0x194228, table, u32::from(item)) }
    }
}

fn available(item: u16) -> u32 {
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
    if unsafe { trade::publish(&API) } {
        menu::install(base);
    }
}
