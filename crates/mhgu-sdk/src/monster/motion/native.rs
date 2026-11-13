//! GU animation state and native motion playback.
use super::MotionId;
use crate::{
    memory::{Field, Object, entry},
    monster::{Monster, MonsterUpdate},
    motion::MotionList,
};

// CF5F64, B0A84, AFF40.
const MOTION: Field<u16> = Field::at(1204);
const MOTION_FLAGS: Field<u8> = Field::at(1206);
const MOTION_FRAME: Field<f32> = Field::at(1280);
const PLAY_MOTION: u32 = 0xaff40;

impl<'a> Monster<'a> {
    /// Borrow a main animation bank from this monster's resource table.
    pub fn motion_list(self, bank: u8) -> Option<MotionList<'a>> {
        if bank >= 16 {
            return None;
        }
        let addr = self.0.get(Field::at(3844 + 4 * usize::from(bank)));
        unsafe { Object::at(addr) }.map(MotionList)
    }
    pub fn motion(self) -> MotionId {
        MotionId::from_raw(self.0.get(MOTION))
    }
    pub fn motion_frame(self) -> f32 {
        self.0.get(MOTION_FRAME)
    }
    pub fn motion_finished(self) -> bool {
        self.0.get(MOTION_FLAGS) & 4 != 0
    }
}

impl MonsterUpdate<'_> {
    /// Start a loaded bank/slot at frame zero. Missing resources and invalid blends return false.
    pub fn play_motion(self, id: MotionId, blend: f32) -> bool {
        let actor = self.monster();
        if !blend.is_finite()
            || actor.state().is_none()
            || !actor
                .motion_list(id.bank())
                .is_some_and(|list| list.contains(id.slot()))
        {
            return false;
        }
        unsafe {
            mhgu_sdk_play_motion(
                actor.as_raw(),
                id.raw().into(),
                entry(PLAY_MOTION) as u32,
                blend.to_bits(),
            );
        }
        true
    }
}

// The Forge target uses soft-float AAPCS. At CF5FBC, GU passes actor/id in r0/r1,
// blend/start frame in s0/s1. Bind this symbol locally in the final NRO.
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global mhgu_sdk_play_motion",
    ".hidden mhgu_sdk_play_motion",
    ".type mhgu_sdk_play_motion,%function",
    "mhgu_sdk_play_motion:",
    "vmov s0,r3",
    "mov r12,#0",
    "vmov s1,r12",
    "bx r2",
);
unsafe extern "C" {
    fn mhgu_sdk_play_motion(actor: u32, motion: u32, function: u32, blend_bits: u32);
}
