//! Native anger gauge, timer and mode transition.
use super::AngerMode;
use crate::{
    memory::{Field, entry},
    monster::{MonsterState, MonsterUpdate},
};

// BCC04 / BCE6C
const ANGER: Field<u16> = Field::at(1296);
const ANGER_TIMER: Field<f32> = Field::at(1300);
const ANGER_MODE: Field<u8> = Field::at(1304);
const SET_ANGER_MODE: u32 = 0xbce6c;

impl MonsterUpdate<'_> {
    /// Enter ordinary rage if the update has a live state block and species parameters.
    pub fn enter_rage(self) {
        let actor = self.monster();
        if actor.state().is_none() || actor.0.get(Field::<u32>::at(30188)) == 0 {
            return;
        }

        let change = unsafe {
            core::mem::transmute::<usize, unsafe extern "C" fn(u32, u32) -> u32>(entry(
                SET_ANGER_MODE,
            ))
        };
        unsafe {
            change(actor.as_raw(), AngerMode::Enraged.raw().into());
        }
    }
}

impl MonsterState<'_> {
    pub fn anger_mode(self) -> AngerMode {
        self.0.get(ANGER_MODE).into()
    }
    pub fn anger_gauge(self) -> u16 {
        self.0.get(ANGER)
    }
    pub fn set_anger_gauge(self, amount: u16) {
        self.0.set(ANGER, amount);
    }
    pub fn set_anger_timer(self, frames: f32) {
        self.0.set(ANGER_TIMER, frames);
    }
}
