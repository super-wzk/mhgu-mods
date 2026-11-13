use super::MotionSlot;
use crate::memory::{Object, entry};

/// A borrowed native rMotionList resource, not a serialized LMT file.
#[derive(Clone, Copy)]
pub struct MotionList<'a>(pub(crate) Object<'a>);

impl MotionList<'_> {
    /// B49E54 rejects missing headers, out-of-range slots and null clip entries.
    pub fn contains(self, slot: MotionSlot) -> bool {
        let lookup: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0xb49e54)) };
        unsafe { lookup(self.0.addr(), slot.0.into()) != 0 }
    }
}
