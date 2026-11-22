use crate::read;
use mhfz_sdk::effects::{MotionClock, world_pose_angles};
use mhgu_sdk::style::{
    action::MotionFrame,
    particle::{self, Anchor, Bank},
};

static BANK: Bank = Bank::new(
    cr"effect\frontier_sns\particles",
    include_bytes!("../target/assets/effects/routes.bin"),
);
const EVENTS: &[[u8; 18]] = include_bytes!("../target/assets/effects/motion_fx.bin")
    .as_chunks()
    .0;
pub unsafe fn preload() {
    unsafe {
        BANK.preload();
    }
}
pub unsafe fn vacuum(pos: [f32; 3], yaw: u16) {
    unsafe {
        BANK.emit(0, Anchor::World(pos, yaw));
    }
}
pub unsafe fn round_force(pos: [f32; 3], reach: f32) {
    unsafe {
        BANK.emit(
            23,
            Anchor::ScaledWorld {
                position: pos,
                angles: [0; 3],
                scale: reach / 100.,
            },
        );
    }
}

fn route(bank: u16, selector: u16) -> Option<usize> {
    let (first, ids): (usize, &[u16]) = match bank {
        4020 => (1, &[0, 4, 5, 9, 11, 12, 13, 17, 18]),
        4035 => (10, &[2, 3, 7]),
        4036 => (13, &[2, 7]),
        _ => return None,
    };
    ids.iter().position(|&s| s == selector).map(|i| first + i)
}
pub unsafe fn update(clock: &mut MotionClock, actor: u32, clip: u16, sample: MotionFrame) {
    unsafe {
        for e in clock.advance(EVENTS, clip, sample) {
            let (model, joint) = match e.node {
                0 => (actor, -1),
                2 => (actor, 0),
                5 => (actor, 16),
                8 => (actor, 19),
                10 => (actor, 2),
                18 => (actor, 12),
                21 => (read(actor, 9120), 0),
                _ => continue,
            };
            if model == 0 {
                continue;
            }
            if matches!(e.kind, 3 | 7) {
                if let (Some(parent), Some(route)) =
                    (particle::parent(model), route(e.bank, e.selector))
                {
                    BANK.emit(
                        route,
                        Anchor::Joint {
                            parent,
                            joint,
                            offset: Some(e.offset),
                        },
                    );
                }
                continue;
            }
            let (mut pos, yaw) = particle::world_offset(actor, e.offset);
            let mut rot = [0, yaw, 0];
            if joint >= 0 {
                let Some(matrix) = particle::joint_matrix(model, joint as u32) else {
                    continue;
                };
                let pose: [f32; 16] = read(matrix, 0);
                pos = particle::transform_offset(&pose, e.offset);
                if matches!(e.mode, 0 | 2) {
                    rot = world_pose_angles(&pose);
                }
            }
            let mut wet = false;
            if matches!(e.kind, 1 | 5) {
                if let Some((floor, water)) = particle::ground_heights(actor, pos) {
                    wet = water.is_some();
                    pos[1] = water.unwrap_or(floor);
                } else if e.kind == 1 {
                    continue;
                }
            }
            if e.mode == 4 {
                rot[1] = yaw.wrapping_add(0x4000);
            }
            let id = if e.kind == 1 {
                if wet {
                    pos[1] += 5.;
                    15
                } else if e.bank == 7 {
                    17
                } else {
                    16
                }
            } else if let Some(route) = route(e.bank + u16::from(wet), e.selector) {
                route
            } else {
                continue;
            };
            BANK.emit(id, Anchor::RotatedWorld(pos, rot));
        }
    }
}

pub unsafe fn trail(actor: u32, channel: usize) {
    unsafe {
        BANK.emit(18 + channel, Anchor::World(read(actor, 64), 0));
    }
}
