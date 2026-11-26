//! Timed w07 blade trails (81F89FAC, types 5 and 6).
use crate::{
    fx,
    peer::{COUNT, Slot},
    read,
};
use core::sync::atomic::{AtomicU32, Ordering};
use mhfz_sdk::{
    sync::Shared,
    trail::{self, Blade, Sample},
};
use mhgu_sdk::weapon::style::action::melee;

const SPAN: f32 = f32::from_le_bytes(*include_bytes!("../target/assets/effects/trail_span.bin"));
static VERSION: AtomicU32 = AtomicU32::new(0);
static POSES: [Shared<Option<(u32, Sample)>>; COUNT * 2] = [const { Shared::new(None) }; COUNT * 2];

unsafe extern "C" fn snapshot<const I: usize>(out: *mut Sample) -> u32 {
    POSES[I].with(|state| match *state {
        Some((version, sample)) => {
            unsafe { out.write(sample) };
            version
        }
        None => 0,
    })
}
pub fn install() {
    unsafe {
        trail::bind(c"effect\\frontier_ls\\m501", snapshot::<0>);
        trail::bind(c"effect\\frontier_ls\\m504", snapshot::<1>);
        trail::bind(c"effect\\frontier_ls\\m505", snapshot::<2>);
        trail::bind(c"effect\\frontier_ls\\m506", snapshot::<3>);
        trail::bind(c"effect\\frontier_ls\\m507", snapshot::<4>);
        trail::bind(c"effect\\frontier_ls\\m508", snapshot::<5>);
        trail::bind(c"effect\\frontier_ls\\m509", snapshot::<6>);
        trail::bind(c"effect\\frontier_ls\\m510", snapshot::<7>);
        trail::bind(c"effect\\frontier_ls\\m511", snapshot::<8>);
        trail::bind(c"effect\\frontier_ls\\m512", snapshot::<9>);
    }
}

#[derive(Default)]
pub struct History {
    strips: [Strip; 2],
}
impl History {
    pub unsafe fn start(&mut self, slot: Slot, actor: u32, frames: f32, strong: bool) {
        unsafe { self.strips[0].start(slot.0, actor, frames, strong) };
    }

    pub unsafe fn swing(&mut self, slot: Slot, actor: u32, frames: f32) {
        // 81F89FAC allocates a separate strip for each window. The first
        // and second windows of a full release coexist for five FZ ticks.
        let main = &self.strips[0];
        let lane =
            usize::from(main.remaining > 0. && main.motion == unsafe { read::<u16>(actor, 1204) });
        unsafe { self.strips[lane].start(slot.0 + lane * COUNT, actor, frames, false) };
    }

    pub unsafe fn tick(&mut self, actor: u32, dt: f32) {
        for strip in &mut self.strips {
            unsafe { strip.tick(actor, dt) };
        }
    }
}

#[derive(Default)]
struct Strip {
    channel: Option<usize>,
    past: [Blade; 6],
    motion: u16,
    remaining: f32,
    fraction: f32,
    emit_in: f32,
    strong: bool,
    remote: bool,
}
impl Strip {
    unsafe fn start(&mut self, channel: usize, actor: u32, frames: f32, strong: bool) {
        if frames <= 0. {
            return;
        }
        self.channel = Some(channel);
        self.remaining = frames;
        self.remote = channel % COUNT != 0;
        self.strong = strong || (!self.remote && unsafe { read::<i32>(actor, 10112) } == 100);
        self.motion = unsafe { read(actor, 1204) };
        self.fraction = 0.;
        self.emit_in = 0.;
        self.past = [Blade::default(); 6];
        unsafe {
            self.tick(actor, 0.);
        }
    }
    pub unsafe fn tick(&mut self, actor: u32, dt: f32) {
        if self.remaining <= 0. {
            return;
        }
        let Some(channel) = self.channel else { return };
        self.remaining -= dt;
        if self.remaining <= 0. || self.motion != unsafe { read::<u16>(actor, 1204) } {
            self.remaining = 0.;
            POSES[channel].with(|p| *p = None);
            return;
        }
        let Some(m) = (unsafe { fx::bone(actor, 8) }) else {
            POSES[channel].with(|p| *p = None);
            return;
        };
        self.fraction += dt;
        if self.fraction >= 2. || self.past[0].length == 0. {
            self.fraction %= 2.;
            let tip = unsafe { melee::equipped_tip(actor, 0, 8) }.unwrap_or(20. + 2.1 * SPAN);
            let Some(blade) = Blade::from_pose(&m, 20., tip) else {
                POSES[channel].with(|p| *p = None);
                return;
            };
            if self.past[0].length == 0. {
                self.past.fill(blade);
            } else {
                self.past.copy_within(0..5, 1);
                self.past[0] = blade;
            }
        }
        let tint = if self.strong {
            [255, 51, 51, 255]
        } else if self.remote {
            // GU's LS netPlAction transmits blade color, not the spirit gauge.
            [255, 255, 255, 192]
        } else {
            let spirit = unsafe { read::<i32>(actor, 10112) };
            let c = 255 - ((spirit - 60).clamp(0, 60) * 204 / 60) as u8;
            [255, c, c, ((spirit - 20).clamp(0, 20) * 255 / 20) as u8]
        };
        let version = VERSION
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        POSES[channel].with(|p| {
            *p = Some((
                version,
                Sample {
                    poses: self.past,
                    tint,
                },
            ))
        });
        self.emit_in -= dt;
        if self.emit_in <= 0. {
            unsafe {
                fx::trail(actor, channel);
                if self.strong {
                    fx::trail(actor, channel);
                }
            }
            self.emit_in = 4.;
        }
    }
}
impl Drop for Strip {
    fn drop(&mut self) {
        if let Some(channel) = self.channel {
            POSES[channel].with(|p| *p = None);
        }
    }
}
