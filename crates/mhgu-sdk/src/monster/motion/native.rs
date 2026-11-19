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
    /// Set the monster's base playback rate and reapply its current action modifier.
    /// GU keeps the primary and secondary animation clocks in sync in B0870.
    pub fn set_base_motion_rate(self, rate: f32) {
        let Some(state) = self.state() else { return };
        let base = Field::<f32>::at(908);
        if !rate.is_finite() || rate <= 0. || state.0.get(base) == rate {
            return;
        }
        state.0.set(base, rate);
        let modifier = state.0.get(Field::<f32>::at(920));
        unsafe {
            mhgu_sdk_refresh_motion_rate(self.as_raw(), modifier.to_bits(), entry(0xb0870) as u32)
        };
    }

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
    /// Effective primary playback rate, including the engine’s action modifier.
    pub fn motion_rate(self) -> f32 {
        self.0.get(Field::at(1292))
    }
    pub fn motion_frame(self) -> f32 {
        self.0.get(MOTION_FRAME)
    }
    /// Remaining native blend time, measured in actor update frames.
    pub fn motion_blend_remaining(self) -> f32 {
        self.0.get(Field::at(1268))
    }
    /// Temporarily override the primary animation clock rate for a native pass.
    /// Zero holds its frame while GU's independent blend clock keeps advancing.
    /// The base rate and the other animation layers are unchanged.
    pub fn with_motion_clock_rate<R>(self, rate: f32, pass: impl FnOnce() -> R) -> R {
        struct Restore<'a>(Monster<'a>, f32);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.0.set(Field::at(1292), self.1);
            }
        }
        if !rate.is_finite() || rate < 0. {
            return pass();
        }
        let _restore = Restore(self, self.motion_rate());
        self.0.set(Field::at(1292), rate);
        pass()
    }
    pub fn motion_finished(self) -> bool {
        self.0.get(MOTION_FLAGS) & 4 != 0
    }
}

impl MonsterUpdate<'_> {
    /// Start a loaded bank/slot at frame zero. Missing resources and invalid blends return false.
    pub fn play_motion(self, id: MotionId, blend: f32) -> bool {
        let actor = self.monster();
        // AFDA4 will ignore playback when locked; 954640 also requires the model.
        let Some(state) = actor.state() else {
            return false;
        };
        if !blend.is_finite()
            || state.0.get(Field::<u8>::at(424)) & 4 != 0
            || actor.0.get(Field::<u32>::at(240)) == 0
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
        actor.motion() == id
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
    ".global mhgu_sdk_refresh_motion_rate",
    ".hidden mhgu_sdk_refresh_motion_rate",
    ".type mhgu_sdk_refresh_motion_rate,%function",
    "mhgu_sdk_refresh_motion_rate:",
    "vmov s0,r1",
    "bx r2",
    "mhgu_sdk_play_motion:",
    "vmov s0,r3",
    "mov r12,#0",
    "vmov s1,r12",
    "bx r2",
);
unsafe extern "C" {
    fn mhgu_sdk_refresh_motion_rate(actor: u32, modifier_bits: u32, function: u32);
    fn mhgu_sdk_play_motion(actor: u32, motion: u32, function: u32, blend_bits: u32);
}
