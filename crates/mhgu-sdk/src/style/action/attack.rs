//! Hit data supplied to GU's weapon attack constructor.

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Attack {
    /// Equipped weapon HDP row supplying the native hit properties and shape.
    pub template: u16,
    pub power: u16,
    /// Delay and lifetime in GU motion frames measured from this event.
    pub delay: f32,
    pub duration: f32,
    /// GU weapon attack flags, passed to the constructor and weapon adjustment.
    pub flags: u32,
    /// Optional loaded rHitShape resource. Zero uses the equipped weapon shape.
    pub shape: u32,
    pub no_sharpness_cost: u32,
}

impl Attack {
    /// Submit a style strike through the same collision and weapon adjustment
    /// path as GU's weapon action descriptors.
    ///
    /// # Safety
    /// The actor and optional shape must remain live during the call on GU's
    /// game thread. The template and flags must describe a valid GU attack.
    pub unsafe fn submit(&self, actor: u32) -> bool {
        unsafe { super::super::registry::submit_attack(actor, self) }
    }
}
