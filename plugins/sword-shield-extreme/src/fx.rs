use crate::read;
use mhfz_sdk::effects::{MotionClock, world_pose_angles};
use mhgu_sdk::geometry::Bearing;
use mhgu_sdk::weapon::style::{
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
                    let offset = Some(e.offset);
                    let anchor = if e.kind == 3 && e.mode == 3 {
                        // FZ 81FD3B8E follows the hunter's yaw, independently
                        // of the joint used to position the emitter.
                        Anchor::JointYaw {
                            parent,
                            joint,
                            offset,
                            yaw: read(actor, 4076),
                        }
                    } else {
                        Anchor::Joint {
                            parent,
                            joint,
                            offset,
                        }
                    };
                    BANK.emit(route, anchor);
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
                if e.kind == 4 && e.mode == 2 {
                    // 817BD21C flattens the joint to its heading first.
                    // 817BCBE8 reverses the forward axis of that flat pose.
                    let horizontal = if pose[0].abs() + pose[2].abs() > 1e-6 {
                        (-pose[2], pose[0])
                    } else {
                        (pose[8], pose[10])
                    };
                    let heading = Bearing::between_xz((0., 0.), horizontal).yaw;
                    rot[1] = heading.offset(0x8000).units();
                } else if matches!(e.mode, 0 | 2) {
                    rot = world_pose_angles(&pose);
                }
            }
            let mut wet = false;
            if matches!(e.kind, 1 | 4 | 5) {
                if let Some((floor, water)) = particle::ground_heights(actor, pos) {
                    // Kind 4 projects to the floor without switching banks.
                    let water = water.filter(|_| e.kind != 4);
                    wet = water.is_some();
                    pos[1] = water.unwrap_or(floor);
                } else if e.kind == 1 {
                    continue;
                }
            }
            if e.mode == 4 {
                rot[1] = yaw.wrapping_add(0x4000);
            } else if e.mode == 2 && e.node == 0 && matches!(e.kind, 5 | 6) {
                // Both 817BD21C and 817BD590 use 817BCBE8 for root mode 2.
                rot[1] = yaw.wrapping_add(0x8000);
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
