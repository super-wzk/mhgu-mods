//! Monster attack tables and setup of the monster's native melee contacts.
mod tables;
use super::{MonsterState, MonsterUpdate};
use crate::{combat::collision::Contact, memory::entry};
pub use tables::{AttackRow, AttackTables};

impl MonsterUpdate<'_> {
    /// Arm the primary contact from the loaded main HDE. GU selects its secondary
    /// resource bank at 1000; that bank is outside this method's contract.
    pub fn arm_primary_attack(self, id: u32) -> bool {
        let actor = self.monster();
        if id >= 1000
            || actor.state().is_none()
            || actor.attack_tables().and_then(|t| t.row(id)).is_none()
        {
            return false;
        }
        let arm: unsafe extern "C" fn(u32, u32, i32, u32) -> i32 =
            unsafe { core::mem::transmute(entry(0x7dc2c)) };
        unsafe { arm(actor.as_raw(), 0, id as i32, 0) == 1 }
    }
}

impl<'a> MonsterState<'a> {
    pub fn primary_contact(self) -> Contact<'a> {
        // GU embeds slot zero in the state block (7DC2C).
        Contact(unsafe { self.0.embedded(2456) })
    }
}
impl AttackRow<'_> {
    /// Configure a contact with GU's ordinary slot-zero melee defaults.
    ///
    /// # Safety
    /// Call during attack setup with a native melee row valid for this motion (not a metadata row).
    pub unsafe fn configure_melee(self, contact: Contact) -> bool {
        let Some(group) = self.shape_group() else {
            return false;
        };
        unsafe {
            let init: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32) -> u32 =
                core::mem::transmute(entry(0x168be8));
            init(
                contact.as_raw(),
                1,
                self.owner.as_raw(),
                self.as_raw(),
                group.resource.addr(),
                group.index.into(),
                23,
                2,
            );
            let bind: unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32 =
                core::mem::transmute(entry(0x1696c4));
            let motion = self.owner.motion();
            bind(contact.as_raw(), motion.raw().into(), 0, 0, 0);
            contact.add_flags(0x210);
        }
        true
    }
}
