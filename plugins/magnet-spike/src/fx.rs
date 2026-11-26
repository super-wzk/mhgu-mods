use crate::program::Mode;
use alloc::vec::Vec;
use mhgu_sdk::{
    sync::Shared,
    weapon::style::particle::{self, Anchor, Bank},
};

static BANK: Bank = Bank::new(
    c"effect\\frontier_ms\\particles",
    include_bytes!("../target/assets/frontier_ms_routes.bin"),
);

#[derive(Clone, Copy)]
pub(super) enum Route {
    Shot = 0,
    Mark = 1,
    SlashFront = 2,
    SlashBack = 3,
    Impact = 4,
    GuardDust = 5,
    GuardSplash = 6,
    SlashSparks = 7,
    ImpactSparks = 8,
}

struct Units {
    sequence: u64,
    live: Vec<(u64, u32)>,
}

static UNITS: Shared<Units> = Shared::new(Units {
    sequence: 0,
    live: Vec::new(),
});

extern "C" fn released(unit: u32) {
    UNITS.with(|units| units.live.retain(|&(_, address)| address != unit));
}

pub(super) fn bind(style: u32) -> bool {
    particle::on_release(style, released)
}

pub(super) unsafe fn preload() {
    unsafe { BANK.preload() };
}

pub(super) unsafe fn guard(actor: u32, mode: Mode) {
    // FZ joint 14 is GU logical joint 8. The EFL stores the sparks' delays
    // and offsets, letting the native emitter follow the weapon as it moves.
    let parent = unsafe { particle::parent(actor) };
    let attached = |route: Route, offset| {
        if let Some(parent) = parent {
            unsafe {
                BANK.emit(
                    route as usize,
                    Anchor::JointPose {
                        parent,
                        joint: 8,
                        offset,
                    },
                );
            }
        }
    };
    if matches!(mode, Mode::Slash) {
        // 8102CDDC effect A, at the first accepted block.
        attached(Route::SlashFront, Some([30., 0., -70.]));
        attached(Route::SlashBack, Some([40., 0., 100.]));
        attached(Route::SlashSparks, None);
        return;
    }

    // 812208B8 samples FZ joint 0, using its pitch and yaw with zero roll.
    // The root is also available before the skeleton's first pose has loaded.
    let root =
        unsafe { particle::joint_matrix(actor, 0).map(|matrix| &*(matrix as *const [f32; 16])) };
    let (burst, ground, angles) = if let Some(root) = root {
        let [pitch, yaw, _] = mhfz_sdk::effects::world_pose_angles(root);
        (
            particle::transform_offset(root, [0., 120., 100.]),
            particle::transform_offset(root, [0., 0., 100.]),
            [pitch, yaw, 0],
        )
    } else {
        let (burst, yaw) = unsafe { particle::world_offset(actor, [0., 120., 100.]) };
        let (ground, _) = unsafe { particle::world_offset(actor, [0., 0., 100.]) };
        (burst, ground, [0, yaw, 0])
    };
    let wet = unsafe { particle::ground_heights(actor, ground) }
        .is_some_and(|(_, water)| water.is_some());
    let surface = if wet {
        Route::GuardSplash
    } else {
        Route::GuardDust
    };
    unsafe {
        BANK.emit(Route::Impact as usize, Anchor::RotatedWorld(burst, angles));
        BANK.emit(surface as usize, Anchor::RotatedWorld(ground, angles));
    }
    attached(Route::ImpactSparks, None);
}

/// Owns the layers submitted for one scene object. All calls and drops occur
/// in the hunter controller, quest preload or world-contact callback.
pub(super) struct Effect {
    id: u64,
    route: Route,
    pending: u32,
}

impl Effect {
    pub(super) fn new(route: Route) -> Self {
        let id = UNITS.with(|units| {
            units.sequence += 1;
            units.sequence
        });
        Self {
            id,
            route,
            pending: u32::MAX,
        }
    }

    pub(super) fn move_to(&mut self, position: [f32; 3]) {
        UNITS.with(|units| {
            for &(id, unit) in &units.live {
                if id == self.id {
                    // The setter does not release units or acquire SDK locks.
                    unsafe { particle::move_world_unit(unit, position) };
                }
            }
        });
        if self.pending != 0 {
            self.pending = unsafe {
                BANK.emit_layers(
                    self.route as usize,
                    Anchor::World(position, 0),
                    self.pending,
                    |_, unit| {
                        UNITS.with(|units| units.live.push((self.id, unit)));
                    },
                )
            };
        }
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        loop {
            let unit = UNITS.with(|units| {
                let index = units.live.iter().position(|&(id, _)| id == self.id)?;
                Some(units.live.swap_remove(index).1)
            });
            let Some(unit) = unit else { break };
            // GU can release a unit synchronously. Drop the lock before stopping it.
            unsafe { particle::stop_unit(unit) };
        }
    }
}
