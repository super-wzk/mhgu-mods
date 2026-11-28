//! Versioned character data stored beside the retail save.
use alloc::collections::BTreeMap;
use serde::{Deserialize, Serialize};

pub(crate) const WEAPONS: usize = mhgu_sdk::weapon::WeaponKind::COUNT;
pub(crate) const SETS: u32 = 40;
const VERSION: u32 = 1;
pub(crate) const INVENTORY_LIMIT: usize = 128;

pub(crate) fn custom_id(id: u32) -> Option<u16> {
    u16::try_from(id).ok().filter(|&id| id >= 10)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct SetStyle {
    pub weapon: u8,
    pub id: u16,
    pub fingerprint: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct OwnedWeapon {
    #[serde(alias = "weapon")]
    pub equipment: u16,
    pub upgrade: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct SetWeapon {
    pub instance: u32,
    pub fingerprint: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct Inventory {
    #[serde(default, skip_serializing_if = "alloc::vec::Vec::is_empty")]
    pub slots: alloc::vec::Vec<u32>,
    pub instances: BTreeMap<u32, OwnedWeapon>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub decorations: BTreeMap<u32, [u16; 3]>,
    pub equipped: Option<u32>,
    pub sets: BTreeMap<u8, SetWeapon>,
}

impl Inventory {
    pub const EMPTY: Self = Self {
        slots: alloc::vec::Vec::new(),
        instances: BTreeMap::new(),
        decorations: BTreeMap::new(),
        equipped: None,
        sets: BTreeMap::new(),
    };

    fn valid(&self) -> bool {
        self.slots.len() <= INVENTORY_LIMIT
            && self.slots.iter().enumerate().all(|(i, id)| *id == 0 || (self.instances.contains_key(id) && !self.slots[..i].contains(id)))
            && self.decorations
            .keys()
            .all(|id| self.instances.contains_key(id))
            && self.instances.len() <= INVENTORY_LIMIT
            && self
                .instances
                .iter()
                .all(|(&id, item)| id != 0 && item.equipment != 0)
            && self
                .equipped
                .is_none_or(|id| self.instances.contains_key(&id))
            && self.sets.iter().all(|(&slot, set)| {
                u32::from(slot) < SETS && self.instances.contains_key(&set.instance)
            })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct CardWeapon {
    pub weapon: OwnedWeapon,
    pub fingerprint: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct DiaryWeapon {
    pub equipment: u16,
    pub milestone: u16,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct GuildCard {
    pub equipment: Option<CardWeapon>,
    pub usage: BTreeMap<u16, [u16; 3]>,
    pub diary: BTreeMap<u64, DiaryWeapon>,
}

impl GuildCard {
    const EMPTY: Self = Self {
        equipment: None,
        usage: BTreeMap::new(),
        diary: BTreeMap::new(),
    };

    fn valid(&self) -> bool {
        self.equipment
            .is_none_or(|entry| entry.weapon.equipment != 0)
            && self
                .usage
                .iter()
                .all(|(&id, counts)| id != 0 && counts.iter().all(|&n| n <= 9999))
            && self.diary.len() <= 10
            && self.diary.values().all(|entry| entry.equipment != 0)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct CharacterData {
    pub weapons: [Option<u16>; WEAPONS],
    pub equipment_sets: BTreeMap<u8, SetStyle>,
    #[serde(default)]
    pub inventory: Inventory,
    #[serde(default)]
    pub guild_card: GuildCard,
}

impl CharacterData {
    fn valid(&self) -> bool {
        self.inventory.valid()
            && self.guild_card.valid()
            && self.weapons.iter().flatten().all(|&id| id >= 10)
            && self.equipment_sets.iter().all(|(&slot, entry)| {
                u32::from(slot) < SETS && usize::from(entry.weapon) < WEAPONS && entry.id >= 10
            })
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub(crate) static CHARACTER: mhgu_sdk::sync::Shared<CharacterData> =
    mhgu_sdk::sync::Shared::new(CharacterData {
        weapons: [None; WEAPONS],
        equipment_sets: BTreeMap::new(),
        inventory: Inventory::EMPTY,
        guild_card: GuildCard::EMPTY,
    });

#[derive(Debug, Deserialize, Serialize)]
struct Save {
    version: u32,
    slot: u8,
    data: CharacterData,
}

impl Save {
    fn decode(bytes: &[u8], slot: u8) -> Option<Self> {
        let file: Self = serde_json::from_slice(bytes).ok()?;
        file.valid(slot).then_some(file)
    }

    fn valid(&self, slot: u8) -> bool {
        self.version == VERSION && self.slot == slot && self.data.valid()
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub(super) use native::{character_loaded, install, preview_weapon};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_character_data_round_trips_and_checks_identity() {
        let mut save = Save {
            version: VERSION,
            slot: 1,
            data: CharacterData::default(),
        };
        save.data.weapons[0] = custom_id(10);
        save.data.equipment_sets.insert(
            39,
            SetStyle {
                weapon: 0,
                id: 65535,
                fingerprint: 42,
            },
        );
        save.data.inventory.instances.insert(
            1,
            OwnedWeapon {
                equipment: 17,
                upgrade: 3,
            },
        );
        save.data.inventory.equipped = Some(1);
        save.data.inventory.sets.insert(
            39,
            SetWeapon {
                instance: 1,
                fingerprint: 42,
            },
        );
        let bytes = serde_json::to_vec(&save).unwrap();
        assert_eq!(Save::decode(&bytes, 1).unwrap().data, save.data);
        assert!(Save::decode(&bytes[..bytes.len() - 1], 1).is_none());
        assert!(Save::decode(&bytes, 0).is_none());
        save.data.inventory.equipped = Some(2);
        assert!(Save::decode(&serde_json::to_vec(&save).unwrap(), 1).is_none());
        save.data.inventory.equipped = Some(1);
        save.data.weapons[0] = Some(6);
        assert!(Save::decode(&serde_json::to_vec(&save).unwrap(), 1).is_none());
        assert_eq!(custom_id(65536), None);
    }
}
