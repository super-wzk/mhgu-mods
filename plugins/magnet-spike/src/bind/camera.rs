use super::{METERS, program};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::{action, hunter, rules::CameraLens},
};

#[derive(Clone, Copy)]
enum Phase {
    Hold,
    FinisherWindup,
    Finisher,
    Released,
}

#[derive(Default)]
struct Lens {
    actor: u32,
    area: u8,
    distance: f32,
    weight: f32,
    fov_offset: f32,
}

static LENS: Shared<Option<Lens>> = Shared::new(None);

pub(crate) fn reset_camera() {
    LENS.with(|state| *state = None);
}

pub(crate) unsafe extern "C" fn camera_lens(actor: u32, elapsed: f32, lens: &mut CameraLens) {
    if !unsafe { hunter::is_alive(actor) } {
        reset_camera();
        return;
    }
    let area = unsafe { hunter::area(actor) };
    let (group, slot) = unsafe { action::current(actor) };
    let phase = match action::custom_id(group, slot) {
        Some(99)
            if METERS.with(|meters| {
                meters.binding.as_ref().is_some_and(|binding| {
                    binding.area == area && binding.target.position().is_some()
                })
            }) =>
        {
            Phase::Hold
        }
        Some(105 | 157) => {
            let motion = unsafe { action::MotionFrame::sample(actor) };
            let frame = if motion.blend <= 0. && !unsafe { action::waiting_to_start(actor) } {
                motion.frame * 0.5
            } else {
                0.
            };
            if frame >= f32::from(program::parameter(29)) {
                Phase::Released
            } else if frame >= f32::from(program::parameter(28)) - 2. {
                Phase::Finisher
            } else {
                Phase::FinisherWindup
            }
        }
        _ => Phase::Released,
    };
    LENS.with(|state| {
        if state
            .as_ref()
            .is_some_and(|state| state.actor != actor || state.area != area)
        {
            *state = None;
        }
        if state.is_none() && matches!(phase, Phase::Released) {
            return;
        }
        let state = state.get_or_insert_with(|| Lens {
            actor,
            area,
            ..Lens::default()
        });
        state.apply(phase, elapsed, lens);
    });
}

impl Lens {
    fn apply(&mut self, phase: Phase, elapsed: f32, lens: &mut CameraLens) {
        let ticks = elapsed.max(0.) * 0.5;
        let peak = f32::from(program::parameter(23));
        let step = f32::from(program::parameter(22)) * ticks;
        let target_fov = match phase {
            Phase::Hold => {
                self.distance = f32::from(program::parameter(25));
                program::parameter(24).into()
            }
            Phase::FinisherWindup => program::parameter(24).into(),
            Phase::Finisher => {
                self.distance = f32::from(program::parameter(27));
                // 81226F42 selects camera row 2. Default FZ layout at 82BA6EC4.
                lens.radius = 490.;
                lens.eye_height = 140.;
                lens.target_height = 170.;
                program::parameter(26).into()
            }
            Phase::Released => lens.fov,
        };
        // 81674F10 samples the current distance weight before advancing it.
        lens.radius += self.distance * self.weight / peak;
        self.weight = if matches!(phase, Phase::Released) {
            (self.weight - step).max(0.)
        } else {
            (self.weight + step).min(peak)
        };
        let target_offset = target_fov - lens.fov;
        self.fov_offset += (target_offset - self.fov_offset) * (1. - libm::powf(0.95, ticks));
        if matches!(phase, Phase::Released) && self.fov_offset.abs() < 0.01 {
            self.fov_offset = 0.;
        }
        lens.fov += self.fov_offset;
    }
}
