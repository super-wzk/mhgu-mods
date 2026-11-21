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
    /// GU weapon attack flags plus the style options below.
    pub flags: u32,
    /// Optional loaded rHitShape resource. Zero uses the equipped weapon shape.
    pub shape: u32,
    pub no_sharpness_cost: u32,
}

impl Attack {
    /// Arm the style registry's deflection check for this blade and contact serial.
    pub const DEFLECTABLE: u32 = 1 << 27;

    /// Use the native second contact slot for a simultaneous paired strike.
    pub const SECONDARY: u32 = 1 << 31;

    /// Disable mounting properties inherited from the HDP row.
    pub const NO_MOUNT: u32 = 1 << 30;

    /// Give this strike zero attacker hitstop when GU resolves its contact.
    pub const NO_ATTACKER_HITSTOP: u32 = 1 << 29;

    /// Submit a style strike through the same collision and weapon adjustment
    /// path as GU's weapon action descriptors.
    ///
    /// # Safety
    /// The actor and optional shape must remain live during the call on GU's
    /// game thread. The template and flags must describe a valid GU attack.
    pub unsafe fn submit(&self, actor: u32) -> bool {
        unsafe { super::super::registry::submit_attack(actor, self) }
    }

    /// Submit a strike and scale its native attacker hitstop factor.
    ///
    /// # Safety
    /// The actor and shape must satisfy the requirements of `submit`.
    /// The scale must be finite and nonnegative.
    pub unsafe fn submit_with_hitstop_scale(&self, actor: u32, scale: f32) -> bool {
        unsafe {
            if !self.submit(actor) {
                return false;
            }
            let hand = usize::from(self.flags & Self::SECONDARY != 0);
            let hit = actor + super::melee::BLADE_CONTACT_OFFSETS[hand];
            // GU 168960 has decoded HDP byte 37. Scale the resulting
            // contact value so 16DB68 still chooses the native reaction.
            let factor = (hit + 108) as *mut u8;
            factor.write((f32::from(factor.read()) * scale + 0.5) as u8);
            true
        }
    }
}
