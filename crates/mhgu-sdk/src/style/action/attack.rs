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
    /// Retail blade flags (162891C[0]) for weapon damage, element/status and affinity.
    pub const WEAPON_MODIFIERS: u32 = 7;

    /// Apply weapon affinity and affinity skills in GU's attack constructor.
    pub const AFFINITY: u32 = 1 << 2;

    /// Arm the style registry's deflection check for this blade and contact serial.
    pub const DEFLECTABLE: u32 = 1 << 27;

    /// Supply GU's standard Punishing Draw KO for rows without conditional KO.
    pub const PUNISHING_DRAW: u32 = 1 << 28;

    const DRAW_KO_SHIFT: u32 = 16;
    const DRAW_KO_FIELD: u32 = 0x7f << Self::DRAW_KO_SHIFT;

    #[doc(hidden)]
    pub const DRAW_KO_MASK: u32 = Self::PUNISHING_DRAW | Self::DRAW_KO_FIELD;

    /// Supply the weapon's conditional Punishing Draw KO when it differs from 30.
    pub const fn punishing_draw(ko: u8) -> u32 {
        assert!(ko > 0 && ko <= 127);
        Self::PUNISHING_DRAW | ((ko as u32) << Self::DRAW_KO_SHIFT)
    }

    #[doc(hidden)]
    pub const fn draw_ko(flags: u32) -> i8 {
        match ((flags & Self::DRAW_KO_FIELD) >> Self::DRAW_KO_SHIFT) as i8 {
            0 => -30,
            ko => -ko,
        }
    }

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

    /// Submit a strike with an explicit GU attacker hitstop factor.
    ///
    /// # Safety
    /// The actor and shape must satisfy the requirements of `submit`.
    pub unsafe fn submit_with_hitstop(&self, actor: u32, factor: u8) -> bool {
        unsafe {
            if !self.submit(actor) {
                return false;
            }
            self.set_attacker_hitstop(actor, factor);
            true
        }
    }

    unsafe fn set_attacker_hitstop(&self, actor: u32, factor: u8) {
        let hand = usize::from(self.flags & Self::SECONDARY != 0);
        let hit = actor + super::melee::BLADE_CONTACT_OFFSETS[hand];
        let factor = if self.flags & Self::NO_ATTACKER_HITSTOP != 0 {
            0
        } else {
            factor
        };
        // GU 168960 has decoded HDP byte 37. Leave the template and all
        // other contact properties intact so 16DB68 selects the reaction.
        unsafe { ((hit + 108) as *mut u8).write(factor) };
    }
}

/// Optional entry for attacks whose base KO buildup differs from the equipped HDP.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod stun {
    use super::Attack;
    use crate::service;
    const NAME: &core::ffi::CStr = c"mhgu::StyleStunAttackV1";
    const MAGIC: u32 = u32::from_le_bytes(*b"STA1");

    #[doc(hidden)]
    #[repr(C)]
    pub struct Service(pub unsafe extern "C" fn(u32, *const Attack, u8) -> u32);

    /// # Safety
    /// Called once by the provider during Forge initialization.
    #[doc(hidden)]
    pub unsafe fn publish(api: &'static Service) -> bool {
        unsafe { service::publish(NAME, MAGIC, api) }
    }

    /// # Safety
    /// Same actor, shape and thread requirements as `Attack::submit`.
    pub unsafe fn submit(actor: u32, attack: &Attack, buildup: u8) -> bool {
        service::on_game_thread()
            && unsafe { service::find::<Service>(NAME, MAGIC) }
                .is_some_and(|api| unsafe { (api.0)(actor, attack, buildup) != 0 })
    }

    /// Submit a strike with its custom KO buildup and GU attacker hitstop.
    ///
    /// # Safety
    /// Same actor, shape and thread requirements as `Attack::submit`.
    pub unsafe fn submit_with_hitstop(actor: u32, attack: &Attack, buildup: u8, factor: u8) -> bool {
        unsafe {
            if !submit(actor, attack, buildup) {
                return false;
            }
            attack.set_attacker_hitstop(actor, factor);
            true
        }
    }
}
