//! Frontier Magnet Spike for GU 1.4.
#![no_std]

mod battle;
mod combat;
mod flight;
mod magnet;
mod motion;
mod program;

use mhgu_sdk::weapon::{
    Element, EquipmentId, EquipmentLevel, EquipmentModel, Weapon, WeaponEquipment, WeaponId,
    registry::{self, WeaponRegistry},
    style::{RegisterError, Style, hud, manual, part_motion, world_contact},
    trade::{self, Material, Recipe},
};

const MAGNET_SPIKE: WeaponId = WeaponId::new(16).unwrap();
const MAGNET_SPIKE_ICON: u8 = 255;

static WEAPON: Weapon = Weapon {
    id: MAGNET_SPIKE,
    ranged: false,
    icon: MAGNET_SPIKE_ICON,
};

static TRIAL_SPIKE: WeaponEquipment = WeaponEquipment {
    id: EquipmentId::new(0x1001).unwrap(),
    weapon: MAGNET_SPIKE,
    rarity: 1,
    elements: [Element::NONE; 2],
    special_skill: 0,
    sound_material: 0,
    sound_request: 0,
};

static STYLE: Style = Style {
    id: 16,
    weapon: MAGNET_SPIKE.style_key(),
    encode: Some(motion::encode),
    decode: Some(motion::decode),
    independent: true,
    art_slots: 3,
    controller: Some(battle::dispatch),
    request: None,
    actions: program::ACTIONS,
    guard: None,
    commands: None,
};

#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::*;

    #[forge::entry]
    fn main() {
        registry::when_ready(register);
    }

    extern "C" fn register() {
        match register_weapon() {
            Ok(()) => {
                forge::log::prelude::info!("Magnet Spike: weapon, icon and trade offers registered")
            }
            Err(error) => forge::log::prelude::warn!("Magnet Spike registration failed: {error:?}"),
        }
    }
}

fn register_weapon() -> Result<(), RegisterError> {
    let registry = WeaponRegistry::current().ok_or(RegisterError::Invalid)?;
    registry.register_icon(MAGNET_SPIKE_ICON, c"GUI\\frontier_magnet_spike\\icon")?;
    registry.register(
        &WEAPON,
        &STYLE,
        c"磁斩锤",
        c"极之型磁斩锤",
        c"磁斩锤专属极之型。\r\n          工房消耗回复药，武器商店可直接购买。",
    )?;
    registry.register_equipment(
        &TRIAL_SPIKE,
        &[EquipmentLevel {
            attack: 100,
            defense: 0,
            affinity: 0,
            element_values: [0; 2],
            slots: 0,
            sharpness_type: 10,
            sharpness_level: 0,
        }],
        c"试制磁斩锤",
        c"用于验证制造和购买的磁斩锤。",
        Some(EquipmentModel {
            archive: c"arc\\frontier\\magnet_spike",
            model: c"weapon\\frontier\\magnet_spike\\wk451",
        }),
    )?;
    if !unsafe { mhgu_sdk::weapon::style::preload::bind(STYLE.id, motion::preload) } {
        return Err(RegisterError::Invalid);
    }
    if !part_motion::bind(STYLE.id, &motion::PARTS)
        || !hud::bind_meter(STYLE.id, magnet::meter)
        || !world_contact::bind_hurt(STYLE.id, magnet::update)
        || !mhgu_sdk::weapon::style::action::melee::bind_strike_contact(
            STYLE.id,
            magnet::melee_contact,
        )
        || !manual::bind(STYLE.id, include_str!("manual.txt"))
    {
        return Err(RegisterError::Invalid);
    }
    trade::register_purchase(TRIAL_SPIKE.id, 100)?;
    trade::register_recipe(
        TRIAL_SPIKE.id,
        Recipe {
            zenny: 0,
            materials: [
                Material {
                    item: 9,
                    quantity: 1,
                },
                Material::EMPTY,
                Material::EMPTY,
                Material::EMPTY,
            ],
        },
    )
}
