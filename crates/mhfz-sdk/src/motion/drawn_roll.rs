//! Direction of a Frontier attack cancel, retained across GU's drawn roll transition.
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhgu_sdk::style::action;

pub struct DrawnRoll {
    clips: [u16; 2],
    saved: AtomicU32,
}

impl DrawnRoll {
    pub const fn new(positive_x: u16, negative_x: u16) -> Self {
        Self {
            clips: [positive_x, negative_x],
            saved: AtomicU32::new(0),
        }
    }

    pub fn remember(&self, actor: u32, relative_yaw: u16) -> u32 {
        // FZ uses facing minus stick yaw. Exactly half a turn selects the negative-X roll.
        let side = match relative_yaw {
            9102..=32768 => 2,
            32769..=56434 => 1,
            _ => 0,
        };
        // Live hunter pointers are word-aligned.
        self.saved.store(actor | side, Relaxed);
        side
    }

    pub fn clear(&self) {
        self.saved.store(0, Relaxed);
    }

    pub fn clear_for(&self, actor: u32) {
        let saved = self.saved.load(Relaxed);
        if saved & !3 == actor {
            let _ = self.saved.compare_exchange(saved, 0, Relaxed, Relaxed);
        }
    }

    /// # Safety
    /// Called by the live hunter's controller. GU can request motion 28 during the transition.
    pub unsafe fn enter(&self, actor: u32, relative_yaw: u16) -> bool {
        self.remember(actor, relative_yaw);
        let accepted = unsafe { action::transition_to(actor, 1, 6) };
        if !accepted {
            self.clear();
        }
        accepted
    }

    /// Resolve GU motion 28. A weapon may also sample native rolls that it didn't initiate.
    ///
    /// # Safety
    /// The actor must be the live hunter whose motion is being encoded.
    pub unsafe fn clip(&self, actor: u32, native_yaw: impl FnOnce() -> Option<u16>) -> Option<u16> {
        if unsafe { action::current(actor) } != (1, 6) {
            return None;
        }
        let saved = self.saved.load(Relaxed);
        let side = if saved & !3 == actor {
            saved & 3
        } else {
            self.remember(actor, native_yaw()?)
        };
        match side {
            1 => Some(self.clips[0]),
            2 => Some(self.clips[1]),
            _ => None,
        }
    }
}
