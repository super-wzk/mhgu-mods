//! Camera shakes driven by Frontier weapon events.
use crate::sync::Shared;

struct Shake {
    actor: u32,
    origin: [f32; 3],
    area: u8,
    frames: f32,
    amplitude: f32,
    duration: f32,
}

static SHAKE: Shared<Option<Shake>> = Shared::new(None);

/// # Safety
/// Call on the game thread with a live hunter. Zero clears the current shake.
pub unsafe fn start(actor: u32, preset: u16) {
    if actor == 0 {
        reset();
        return;
    }
    let (amplitude, duration) = match preset {
        0 => (4., 10.),
        1 => (10., 20.),
        2 => (12., 30.),
        11 => (2., 10.),
        _ => return,
    };
    unsafe {
        SHAKE.with(|state| {
            *state = Some(Shake {
                actor,
                origin: ((actor + 64) as *const [f32; 3]).read_unaligned(),
                area: ((actor + 4180) as *const u8).read(),
                frames: 2. * duration,
                duration,
                amplitude,
            });
        });
    }
}

pub fn reset() {
    SHAKE.with(|state| *state = None);
}

/// # Safety
/// Both the hunter and the eye position must remain valid during this call.
pub unsafe extern "C" fn offset(actor: u32, elapsed: f32, eye: *const [f32; 3]) -> f32 {
    SHAKE.with(|state| {
        let Some(shake) = state.as_mut().filter(|shake| shake.actor == actor) else {
            return 0.;
        };
        if unsafe { ((actor + 4180) as *const u8).read() } != shake.area {
            *state = None;
            return 0.;
        }
        shake.frames -= elapsed.max(0.);
        let ticks = (shake.frames.max(0.) * 0.5) as u32;
        if ticks == 0 {
            *state = None;
            return 0.;
        }
        let eye = unsafe { eye.read_unaligned() };
        let distance = libm::sqrtf(
            (0..3)
                .map(|i| (eye[i] - shake.origin[i]) * (eye[i] - shake.origin[i]))
                .sum(),
        );
        // FZ 8166E270 uses range row zero at 8254C956 and timer bit one for the sign.
        let amplitude = shake.amplitude * (1. - distance / 1000.).max(0.);
        let offset =
            amplitude * libm::sinf(ticks as f32 * core::f32::consts::FRAC_PI_2 / shake.duration);
        if ticks & 2 == 0 { offset } else { -offset }
    })
}
