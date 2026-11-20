//! GU blade contacts and the style registry's deflection service.
use crate::{
    memory::{Field, Object},
    service,
};

/// Offsets from the GU hunter to the primary and secondary blade contacts.
pub const BLADE_CONTACT_OFFSETS: [u32; 2] = [12320, 12504];

/// Reset GU's full-stop and slow-hit timers on an accepted recoil transition.
///
/// # Safety
/// The actor must be the live local hunter on the game thread.
pub unsafe fn clear_hitstop(actor: u32) {
    unsafe {
        ((actor + 10156) as *mut f32).write(0.);
        ((actor + 10160) as *mut f32).write(0.);
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
