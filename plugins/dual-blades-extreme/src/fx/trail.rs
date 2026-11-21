//! Skin the Frontier blade strip from 30 Hz hand poses. GU submits one
//! indexed model draw and owns the texture, blend, camera and effect life.
use crate::read;

use mhfz_sdk::{
    sync::Shared,
    trail::{self, Blade, Sample},
};
use mhgu_sdk::style::{action::melee, particle};
unsafe fn sample_blade(actor: u32, hand: usize, joint: u8) -> Option<Blade> {
    unsafe {
        let tip_z = melee::equipped_tip(actor, hand, joint)?;
        let matrix = particle::joint_matrix(actor, u32::from(joint))?;
        Blade::from_pose(&read(matrix, 0), 15., tip_z)
    }
}

#[derive(Clone, Copy, Default)]
struct Poses {
    hands: [[Blade; 6]; 2],
    swing_hands: [[Blade; 6]; 2],
    active: [bool; 5], // sharpening, two demon trails, two timed swing trails
}

#[derive(Default)]
pub(super) struct History {
    pub(super) slot: usize,
    poses: Poses,
    area: Option<u8>,
    fraction: f32,
    sharpen_time: f32,
    sharpen_motion: u16,
    swing_time: f32,
    swing_motion: u16,
}

// Effect generation runs with the hunter. Draw submission can run on a
// rendering worker. Never keep a hunter or animation pointer in the packet.
const EMPTY_HANDS: [[Blade; 6]; 2] = [[Blade {
    base: [0.; 3],
    x: [0.; 3],
    y: [0.; 3],
    length: 0.,
}; 6]; 2];
const EMPTY: Poses = Poses {
    hands: EMPTY_HANDS,
    swing_hands: EMPTY_HANDS,
    active: [false; 5],
};
static POSES: [Shared<(u32, Poses)>; 5] = [const { Shared::new((0, EMPTY)) }; 5];

fn publish(slot: usize, poses: &Poses) {
    POSES[slot].with(|(version, state)| {
        *version = version.wrapping_add(1).max(1);
        *state = *poses;
    });
}

impl History {
    pub(super) fn clear_action(&mut self) {
        if self.sharpen_time == 0. && self.swing_time == 0. {
            return;
        }
        self.sharpen_time = 0.;
        self.swing_time = 0.;
        self.poses.active[0] = false;
        self.poses.active[3..].fill(false);
        unsafe { super::stop_swings(self.slot) };
        publish(self.slot, &self.poses);
    }

    pub(super) unsafe fn sharpen(&mut self, actor: u32, phase: u8, ticks: u16) {
        self.sharpen_time = 2. * f32::from(ticks);
        self.sharpen_motion = unsafe { read(actor, 1204) };
        unsafe {
            self.sample(actor, phase, 0.);
        }
    }

    pub(super) unsafe fn swing(&mut self, actor: u32, phase: u8, frames: f32) {
        self.swing_time = frames;
        self.swing_motion = unsafe { read(actor, 1204) };
        self.poses.swing_hands = EMPTY_HANDS;
        unsafe { self.sample(actor, phase, 0.) };
    }

    pub(super) unsafe fn sample(&mut self, actor: u32, phase: u8, dt: f32) {
        unsafe {
            let motion = read::<u16>(actor, 1204);
            if motion != self.sharpen_motion {
                self.sharpen_time = 0.;
            }
            if self.swing_time > 0. {
                self.swing_time = if motion == self.swing_motion {
                    (self.swing_time - dt).max(0.)
                } else {
                    0.
                };
                if self.swing_time == 0. {
                    super::stop_swings(self.slot);
                }
            }
            self.sharpen_time = (self.sharpen_time - dt).max(0.);
            let sharpening = self.sharpen_time > 0.;
            let demon = phase != 0;
            let swinging = self.swing_time > 0.;
            if sharpening || demon || swinging {
                let area = read(actor, 4180);
                let reset = self.area != Some(area) || dt > 8.;
                self.fraction += dt;
                let advance = self.fraction >= 2.;
                if advance {
                    self.fraction %= 2.;
                }
                for (hand, joint) in super::BLADE_JOINTS.into_iter().enumerate() {
                    let blade = sample_blade(actor, hand, joint);
                    if sharpening || demon {
                        update_hand(&mut self.poses.hands[hand], blade, reset, advance);
                    }
                    if swinging {
                        update_hand(&mut self.poses.swing_hands[hand], blade, reset, advance);
                    }
                }
                self.area = Some(area);
                let [right_ready, left_ready] =
                    self.poses.hands.each_ref().map(|past| past[0].length > 0.);
                self.poses.active = [
                    sharpening && right_ready,
                    demon && right_ready,
                    demon && left_ready,
                    swinging && self.poses.swing_hands[0][0].length > 0.,
                    swinging && self.poses.swing_hands[1][0].length > 0.,
                ];
            } else if self.area.take().is_some() {
                self.poses.active = [false; 5];
                self.fraction = 0.;
            } else {
                return;
            }
            publish(self.slot, &self.poses);
        }
    }
}

fn update_hand(past: &mut [Blade; 6], blade: Option<Blade>, reset: bool, advance: bool) {
    let Some(blade) = blade else {
        past[0].length = 0.;
        return;
    };
    if reset || past[0].length == 0. {
        past.fill(blade);
    } else if advance {
        past.copy_within(0..5, 1);
        past[0] = blade;
    }
}

impl Drop for History {
    fn drop(&mut self) {
        unsafe { super::stop_swings(self.slot) };
        publish(self.slot, &EMPTY);
    }
}

unsafe extern "C" fn snapshot<const SLOT: usize, const CHANNEL: usize>(out: *mut Sample) -> u32 {
    POSES[SLOT].with(|(version, state)| {
        if !state.active[CHANNEL] {
            return 0;
        }
        let poses = match CHANNEL {
            0 | 1 => state.hands[0],
            2 => state.hands[1],
            _ => state.swing_hands[CHANNEL - 3],
        };
        unsafe {
            out.write(Sample {
                poses,
                tint: [255; 4],
            })
        };
        *version
    })
}

pub(super) fn install() {
    unsafe fn bind<const SLOT: usize>(paths: [&'static core::ffi::CStr; 5]) {
        unsafe {
            trail::bind(paths[0], snapshot::<SLOT, 0>);
            trail::bind(paths[1], snapshot::<SLOT, 1>);
            trail::bind(paths[2], snapshot::<SLOT, 2>);
            trail::bind(paths[3], snapshot::<SLOT, 3>);
            trail::bind(paths[4], snapshot::<SLOT, 4>);
        }
    }
    unsafe {
        bind::<0>([
            c"effect\\frontier_db\\m501",
            c"effect\\frontier_db\\m502",
            c"effect\\frontier_db\\m503",
            c"effect\\frontier_db\\m504",
            c"effect\\frontier_db\\m505",
        ]);
        bind::<1>([
            c"effect\\frontier_db\\m511",
            c"effect\\frontier_db\\m512",
            c"effect\\frontier_db\\m513",
            c"effect\\frontier_db\\m514",
            c"effect\\frontier_db\\m515",
        ]);
        bind::<2>([
            c"effect\\frontier_db\\m521",
            c"effect\\frontier_db\\m522",
            c"effect\\frontier_db\\m523",
            c"effect\\frontier_db\\m524",
            c"effect\\frontier_db\\m525",
        ]);
        bind::<3>([
            c"effect\\frontier_db\\m531",
            c"effect\\frontier_db\\m532",
            c"effect\\frontier_db\\m533",
            c"effect\\frontier_db\\m534",
            c"effect\\frontier_db\\m535",
        ]);
        bind::<4>([
            c"effect\\frontier_db\\m541",
            c"effect\\frontier_db\\m542",
            c"effect\\frontier_db\\m543",
            c"effect\\frontier_db\\m544",
            c"effect\\frontier_db\\m545",
        ]);
    }
}
