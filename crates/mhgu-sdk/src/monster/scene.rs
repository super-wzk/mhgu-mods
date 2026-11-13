//! The quest services available during a monster update and its patrol selection.
use super::{MonsterState, MonsterUpdate};
use crate::{memory::Field, quest::Quest};
use alloc::vec::Vec;

const PATROL_MASK_INDEX: Field<u32> = Field::at(30256);
const TARGET_PLAYER: Field<u8> = Field::at(564);

impl MonsterState<'_> {
    pub fn target_player(self) -> u8 {
        self.0.get(TARGET_PLAYER)
    }
}

impl<'a> MonsterUpdate<'a> {
    pub fn quest(self) -> Quest<'a> {
        // MonsterUpdate guarantees an active quest on this game thread.
        unsafe { Quest::current() }
    }

    /// Native monster patrol predicates from B6DA0, in area-node order.
    pub fn patrol_points(self) -> Vec<[f32; 3]> {
        let actor = self.monster();
        let Some(area) = self.quest().area(actor.area()) else {
            return Vec::new();
        };
        let mask = 1u32
            .checked_shl(actor.0.get(PATROL_MASK_INDEX))
            .unwrap_or(0);
        area.nodes()
            .filter(|node| node.flags() & 0x10 != 0 && node.mask() & mask != 0)
            .map(|node| node.position())
            .collect()
    }
}
