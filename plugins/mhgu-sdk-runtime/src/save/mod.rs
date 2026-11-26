//! Versioned character data stored beside the retail save.
use alloc::collections::BTreeMap;
use serde::{Deserialize, Serialize};

pub(crate) const WEAPONS: usize = 15;
pub(crate) const SETS: u32 = 40;
const VERSION: u32 = 1;

pub(crate) fn custom_id(id: u32) -> Option<u16> {
    u16::try_from(id).ok().filter(|&id| id >= 10)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct SetStyle {
    pub weapon: u8,
    pub id: u16,
    pub fingerprint: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct Styles {
    pub weapons: [Option<u16>; WEAPONS],
    pub equipment_sets: BTreeMap<u8, SetStyle>,
}

impl Styles {
    fn valid(&self) -> bool {
        self.weapons.iter().flatten().all(|&id| id >= 10)
            && self.equipment_sets.iter().all(|(&slot, entry)| {
                u32::from(slot) < SETS && usize::from(entry.weapon) < WEAPONS && entry.id >= 10
            })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Save {
    version: u32,
    slot: u8,
    generation: u32,
    data: Styles,
}

impl Save {
    fn decode(bytes: &[u8], slot: u8, parity: u32) -> Option<Self> {
        let file: Self = serde_json::from_slice(bytes).ok()?;
        (file.version == VERSION
            && file.slot == slot
            && file.generation != 0
            && file.generation % 2 == parity
            && file.data.valid())
        .then_some(file)
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub(super) use native::install;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_character_data_round_trips_and_checks_identity() {
        let mut save = Save {
            version: VERSION,
            slot: 1,
            generation: 3,
            data: Styles::default(),
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
        let bytes = serde_json::to_vec(&save).unwrap();
        assert_eq!(Save::decode(&bytes, 1, 1).unwrap().data, save.data);
        assert!(Save::decode(&bytes[..bytes.len() - 1], 1, 1).is_none());
        assert!(Save::decode(&bytes, 0, 1).is_none());
        assert!(Save::decode(&bytes, 1, 0).is_none());
        save.data.weapons[0] = Some(6);
        assert!(Save::decode(&serde_json::to_vec(&save).unwrap(), 1, 1).is_none());
        assert_eq!(custom_id(65536), None);
    }
}
