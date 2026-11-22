//! FZ 81F89FAC type 1 samples the sword wrist for eight ticks on the returning slash.
use crate::{fx, read};
use core::sync::atomic::{AtomicU32, Ordering};
use mhfz_sdk::{
    sync::Shared,
    trail::{self, Blade, Sample},
};
use mhgu_sdk::style::{action::melee, particle, remote::PLAYER_COUNT};

const SPAN: f32 = f32::from_le_bytes(*include_bytes!("../target/assets/effects/trail_span.bin"));
static VERSION: AtomicU32 = AtomicU32::new(0);
static POSES: [Shared<Option<(u32, Sample)>>; 1 + PLAYER_COUNT] =
    [const { Shared::new(None) }; 1 + PLAYER_COUNT];

unsafe extern "C" fn snapshot<const SLOT: usize>(out: *mut Sample) -> u32 {
    POSES[SLOT].with(|pose| match *pose {
        Some((version, sample)) => {
            unsafe { out.write(sample) };
            version
        }
        None => 0,
    })
}
pub fn install() {
    unsafe {
        trail::bind(cr"effect\frontier_sns\m501", snapshot::<0>);
        trail::bind(cr"effect\frontier_sns\m504", snapshot::<1>);
        trail::bind(cr"effect\frontier_sns\m505", snapshot::<2>);
        trail::bind(cr"effect\frontier_sns\m506", snapshot::<3>);
        trail::bind(cr"effect\frontier_sns\m507", snapshot::<4>);
    }
}
#[derive(Default)]
pub struct History {
    slot: Option<usize>,
    remaining: f32,
    next_pose: f32,
    next_emit: f32,
    past: [Blade; 6],
}
impl History {
    pub fn clear(&mut self) {
        if let Some(slot) = self.slot.take() {
            POSES[slot].with(|p| *p = None);
        }
    }
    pub unsafe fn start(&mut self, slot: usize, actor: u32, frames: f32) {
        self.slot = Some(slot);
        self.remaining = frames;
        self.next_pose = 0.;
        self.next_emit = 0.;
        self.past = [Blade::default(); 6];
        unsafe { self.tick(actor, 0.) };
    }
    pub unsafe fn tick(&mut self, actor: u32, dt: f32) {
        let Some(slot) = self.slot else { return };
        self.remaining -= dt;
        if self.remaining <= 0. {
            self.clear();
            return;
        }
        self.next_pose -= dt;
        if self.next_pose <= 0. {
            let Some(matrix) = (unsafe { particle::joint_matrix(actor, 8) }) else {
                return;
            };
            let tip = unsafe { melee::equipped_tip(actor, 0, 8) }.unwrap_or(14. + SPAN);
            let Some(blade) = Blade::from_pose(&unsafe { read(matrix, 0) }, 14., tip) else {
                return;
            };
            if self.past[0].length == 0. {
                self.past.fill(blade);
            } else {
                self.past.copy_within(0..5, 1);
                self.past[0] = blade;
            }
            let version = VERSION
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_add(1)
                .max(1);
            POSES[slot].with(|p| {
                *p = Some((
                    version,
                    Sample {
                        poses: self.past,
                        tint: [255; 4],
                    },
                ))
            });
            self.next_pose = 2. + self.next_pose % 2.;
        }
        self.next_emit -= dt;
        if self.next_emit <= 0. {
            unsafe { fx::trail(actor, slot) };
            self.next_emit = 4.;
        }
    }
}
impl Drop for History {
    fn drop(&mut self) {
        self.clear();
    }
}
