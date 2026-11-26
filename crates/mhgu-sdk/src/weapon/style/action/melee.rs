//! GU blade contacts and the style registry's deflection service.
use crate::{
    combat::collision::HitShape,
    memory::{Field, Object},
    service,
};

/// Offsets from the GU hunter to the primary and secondary blade contacts.
pub const BLADE_CONTACT_OFFSETS: [u32; 2] = [12320, 12504];

const FULL_HITSTOP: u32 = 10156;
const SLOW_HITSTOP: u32 = 10160;

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum DamageKind {
    Slash = 2,
    Impact = 1,
}

/// Authored motion power and the bonus percentage captured when the strike starts.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StrikePower {
    pub motion: u16,
    pub bonus: u8,
}

impl StrikePower {
    /// Truncate motion power after applying affinity, then apply the bonus.
    pub fn adjusted(self, affinity: f32) -> u16 {
        let power = (f32::from(self.motion) * affinity) as u16;
        let percent = 100 + u32::from(self.bonus);
        (u32::from(power) * percent / 100).min(u16::MAX.into()) as u16
    }
}

/// A melee strike authored for an independent weapon.
#[repr(C)]
pub struct Strike {
    pub delay: f32,
    pub duration: f32,
    pub power: StrikePower,
    pub damage: DamageKind,
    pub stun: u8,
    pub hitstop: u8,
    /// Let the registered style's deflection rule classify the first contact.
    pub deflectable: bool,
}

impl Strike {
    /// Submit a compound shape through GU's hit constructor.
    /// The game-thread service validates the hunter identity and current weapon action.
    pub fn submit(&self, actor: u32, shape: &HitShape) -> bool {
        service::on_game_thread()
            && unsafe { service::find::<StrikeService>(STRIKE, STRIKE_MAGIC) }
                .is_some_and(|api| unsafe { (api.submit)(actor, self, shape.as_raw()) != 0 })
    }
}

const STRIKE: &core::ffi::CStr = c"mhgu::IndependentMeleeV5";
const STRIKE_MAGIC: u32 = u32::from_le_bytes(*b"IML5");

/// Called for a local registered strike during native monster damage calculation.
/// `power` contains the authored motion power and its per-strike bonus.
/// `affinity` is GU's resolved damage multiplier (1.0 for a noncritical hit).
pub type StrikeContact = extern "C" fn(actor: u32, power: StrikePower, affinity: f32);

#[doc(hidden)]
#[repr(C)]
pub struct StrikeService {
    pub submit: unsafe extern "C" fn(u32, *const Strike, u32) -> u32,
    pub bind: extern "C" fn(u32, StrikeContact) -> bool,
}

/// Observe monster contacts from this style's independent strikes.
pub fn bind_strike_contact(id: u32, callback: StrikeContact) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<StrikeService>(STRIKE, STRIKE_MAGIC) }
            .is_some_and(|api| (api.bind)(id, callback))
}

/// # Safety
/// Publish once at registry initialization on GU's game thread.
#[doc(hidden)]
pub unsafe fn publish_strike(api: &'static StrikeService) -> bool {
    unsafe { service::publish(STRIKE, STRIKE_MAGIC, api) }
}

/// Whether GU's full-stop or slow-hit timer is active (29BEF4).
///
/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn hitstop_active(actor: u32) -> bool {
    unsafe {
        ((actor + FULL_HITSTOP) as *const f32).read() > 0.
            || ((actor + SLOW_HITSTOP) as *const f32).read() > 0.
    }
}

/// Reset GU's full-stop and slow-hit timers on an accepted recoil transition.
///
/// # Safety
/// The actor must be the live local hunter on the game thread.
pub unsafe fn clear_hitstop(actor: u32) {
    unsafe {
        ((actor + FULL_HITSTOP) as *mut f32).write(0.);
        ((actor + SLOW_HITSTOP) as *mut f32).write(0.);
    }
}

const DEFLECTION: &core::ffi::CStr = c"mhgu::BladeDeflectionV1";
const DEFLECTION_MAGIC: u32 = u32::from_le_bytes(*b"BDF1");

#[doc(hidden)]
#[repr(C)]
pub struct DeflectionService(pub unsafe extern "C" fn(u32) -> bool);

/// # Safety
/// Publish only at registry initialization on GU's game thread.
pub unsafe fn publish_deflection(api: &'static DeflectionService) -> bool {
    unsafe { service::publish(DEFLECTION, DEFLECTION_MAGIC, api) }
}

/// Consume the first hard contact of an opted-in style strike.
///
/// # Safety
/// The actor must be the live local hunter during its style controller tick.
pub unsafe fn take_deflection(actor: u32) -> bool {
    use core::sync::atomic::{AtomicPtr, Ordering};
    // Published service descriptors remain live until process exit.
    static API: AtomicPtr<DeflectionService> = AtomicPtr::new(core::ptr::null_mut());
    let api = match unsafe { API.load(Ordering::Acquire).as_ref() } {
        Some(api) => api,
        None => {
            let Some(api) =
                (unsafe { service::find::<DeflectionService>(DEFLECTION, DEFLECTION_MAGIC) })
            else {
                return false;
            };
            API.store(core::ptr::from_ref(api).cast_mut(), Ordering::Release);
            api
        }
    };
    unsafe { (api.0)(actor) }
}

/// Read the equipped blade's tip along the drawn joint's local Z axis.
/// # Safety
/// The caller selects a live weapon part (hand 0 or 1). The actor and its equipped weapon parts must be live GU objects.
pub unsafe fn equipped_tip(actor: u32, hand: usize, joint: u8) -> Option<f32> {
    unsafe {
        // GU 288248/3084C4 bind parts 7 and 8 to their drawn joints.
        let weapon = Object::at(actor)?.get::<u32>(Field::at(9120 + 4 * hand));
        let weapon = Object::at(weapon)?;
        let model = Object::at(weapon.get(Field::at(240)))?;
        // B490CC loads the equipped MOD's local max Z at rModel + 216.
        let mut tip_z = model.get::<f32>(Field::at(216));
        if weapon.get::<u32>(Field::at(5068)) == u32::from(joint) {
            tip_z = tip_z * weapon.get::<f32>(Field::at(104)) + weapon.get::<f32>(Field::at(72));
        }
        (tip_z.is_finite() && tip_z > 0.).then_some(tip_z)
    }
}

pub type ContactCallback = unsafe extern "C" fn(actor: u32);
#[doc(hidden)]
#[repr(C)]
pub struct ContactService(pub unsafe extern "C" fn(u32, ContactCallback));
const CONTACT: &core::ffi::CStr = c"mhgu::StyleBladeContactV1";
const CONTACT_MAGIC: u32 = u32::from_le_bytes(*b"BCT1");

/// # Safety
/// The registry publishes this resident service once on the game thread.
pub unsafe fn publish_contact(api: &'static ContactService) -> bool {
    unsafe { service::publish(CONTACT, CONTACT_MAGIC, api) }
}
/// # Safety
/// The callback must remain valid for the lifetime of the registered style.
pub unsafe fn bind_contact(id: u32, callback: ContactCallback) -> bool {
    if service::on_game_thread()
        && let Some(api) = unsafe { service::find::<ContactService>(CONTACT, CONTACT_MAGIC) }
    {
        unsafe { (api.0)(id, callback) };
        return true;
    }
    false
}
