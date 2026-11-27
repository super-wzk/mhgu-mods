//! Frontier Magnet Spike for GU 1.4.
#![no_std]

extern crate alloc;

mod battle;
mod bind;
mod combat;
mod controls;
mod flight;
mod fx;
mod magnet;
mod motion;
mod native_media;
mod peer;
mod program;
mod shock;
mod sound;

use mhgu_sdk::weapon::{
    Element, EquipmentId, EquipmentLevel, EquipmentModel, Weapon, WeaponEquipment, WeaponId,
    registry::{self, WeaponRegistry},
    style::{RegisterError, Style, hud, manual, part_motion, remote, rules, world_contact},
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

const fn trial_level(
    attack: i16,
    sharpness_type: u8,
    sharpness_level: u8,
    slots: u8,
) -> EquipmentLevel {
    EquipmentLevel {
        attack,
        sharpness_type,
        sharpness_level,
        slots,
        defense: 0,
        affinity: 0,
        element_values: [0; 2],
    }
}

// Preserve existing instances at Lv.1. Subsequent rows follow GU w07's
// raw weapon line 1, levels 5 through 11, including its sharpness and slots.
static TRIAL_LEVELS: [EquipmentLevel; 8] = [
    trial_level(100, 10, 0, 0),
    trial_level(110, 17, 4, 0),
    trial_level(140, 13, 6, 1),
    trial_level(170, 16, 8, 1),
    trial_level(210, 18, 8, 1),
    trial_level(240, 68, 8, 2),
    trial_level(290, 15, 10, 2),
    trial_level(320, 14, 10, 3),
];

const POTION_RECIPE: Recipe = Recipe {
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
    guard: Some(battle::guard_policy),
    commands: None,
};

static RULES: rules::Rules = rules::Rules {
    // FZ 81813350 reads w13's 90-degree half-angle at mhfdat[0x65c].
    ground_motion: Some(motion::finish_sheathe),
    guard_half_angle: 0x4000,
    deflection: Some(mhfz_sdk::combat::deflection),
    camera_offset: Some(mhfz_sdk::camera::offset),
    camera_yaw: Some(bind::camera_yaw),
    camera_lens: Some(bind::camera_lens),
    ..rules::Rules::EMPTY
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
        &TRIAL_LEVELS,
        c"试制磁斩锤",
        c"使用回复药即可逐级强化，最高 Lv.8。",
        Some(EquipmentModel {
            archive: c"arc\\frontier\\magnet_spike",
            model: c"weapon\\frontier\\magnet_spike\\wk451",
        }),
    )?;
    if !unsafe { mhgu_sdk::weapon::style::preload::bind(STYLE.id, motion::preload) } {
        return Err(RegisterError::Invalid);
    }
    if !part_motion::bind(STYLE.id, &motion::PARTS)
        || !rules::bind(STYLE.id, &RULES)
        || !mhgu_sdk::weapon::style::input::bind_filter(STYLE.id, controls::poll)
        || !unsafe { remote::bind_playback(STYLE.id, &peer::PLAYBACK) }
        || !remote::bind_position_state(STYLE.id, &peer::POSITION)
        || !remote::bind_position_events(STYLE.id, magnet::network::receive)
        || !remote::bind_target_events(STYLE.id, magnet::network::receive_target)
        || !fx::bind(STYLE.id)
        || !shock::bind()
        || !bind::bind()
        || !hud::bind_meter(STYLE.id, magnet::meter)
        || !world_contact::bind_attack(STYLE.id, battle::evade_contact)
        || !world_contact::bind_hurt(STYLE.id, magnet::update)
        || !mhgu_sdk::weapon::style::action::melee::bind_strike_contact(
            STYLE.id,
            magnet::strike_contact,
        )
        || !manual::bind(STYLE.id, include_str!("manual.txt"))
    {
        return Err(RegisterError::Invalid);
    }
    mhfz_sdk::effects::model::install();
    trade::register_purchase(TRIAL_SPIKE.id, 100)?;
    trade::register_recipe(TRIAL_SPIKE.id, POTION_RECIPE)?;
    for level in 0..TRIAL_LEVELS.len() - 1 {
        trade::register_upgrade(TRIAL_SPIKE.id, level as u16, POTION_RECIPE)?;
    }
    Ok(())
}
