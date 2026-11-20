//! Frontier guard and spirit-release camera presets, evaluated on the quest view.
use crate::{read, write};
use mhfz_sdk::sync::Shared;

struct Shake {
    actor: u32,
    origin: [f32; 3],
    area: u8,
    frames: f32,
    amplitude: f32,
    duration: f32,
}
static SHAKE: Shared<Option<Shake>> = Shared::new(None);

pub(super) fn reset() {
    SHAKE.with(|s| *s = None);
}

pub(super) unsafe fn start(actor: u32, preset: u16) {
    let (amplitude, duration) = match preset {
        0 => (4., 10.),
        1 => (10., 20.),
        2 => (12., 30.),
        11 => (2., 10.),
        _ => return,
    };
    unsafe {
        SHAKE.with(|s| {
            *s = Some(Shake {
                actor,
                origin: read(actor, 64),
                area: read(actor, 4180),
                frames: 2. * duration,
                duration,
                amplitude,
            })
        });
    }
}

// 21A90 calls 898AC0 through the one-word branch at 1C334. Forge copies
// two ARM words into its trampoline, so hook the destination's prologue.
#[forge::hook(offset = 0x898ac0)]
unsafe fn view(camera: u32) -> u32 {
    unsafe {
        let offset = SHAKE.with(|state| {
            let Some(shake) = state.as_mut() else {
                return 0.;
            };
            // Other camera types also finalize here. Only uQuestCamera
            // (constructor 1FA00) supplies the tracked hunter at +2140.
            if read::<u32>(camera, 0) != forge::mem::text_addr() + 0x1730d0c
                || read::<u32>(camera, 2140) != shake.actor
            {
                return 0.;
            }
            if read::<u8>(shake.actor, 4180) != shake.area {
                *state = None;
                return 0.;
            }
            shake.frames -= read::<f32>(camera, 28).max(0.);
            let ticks = (shake.frames.max(0.) * 0.5) as u32;
            if ticks == 0 {
                *state = None;
                return 0.;
            }
            let eye: [f32; 3] = read(camera, 64);
            let distance = libm::sqrtf(
                (0..3)
                    .map(|i| (eye[i] - shake.origin[i]) * (eye[i] - shake.origin[i]))
                    .sum(),
            );
            // 8166E270, range row zero at 8254C956, sign from timer bit one.
            let amplitude = shake.amplitude * (1. - distance / 1000.).max(0.);
            let offset = amplitude
                * libm::sinf(ticks as f32 * core::f32::consts::FRAC_PI_2 / shake.duration);
            if ticks & 2 == 0 { offset } else { -offset }
        });
        if offset == 0. {
            return original!(camera);
        }
        let eye: f32 = read(camera, 68);
        let target: f32 = read(camera, 100);
        write(camera, 68, eye + offset);
        write(camera, 100, target + offset);
        let result = original!(camera);
        write(camera, 68, eye);
        write(camera, 100, target);
        result
    }
}
pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), view);
}
