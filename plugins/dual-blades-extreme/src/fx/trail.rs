//! Skin the Frontier blade strip from 30 Hz hand poses. GU submits one
//! indexed model draw and owns the texture, blend, camera and effect life.
use crate::{native, read, write};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};

type Point = [f32; 3];

#[derive(Clone, Copy, Default)]
struct Blade {
    base: Point,
    x: Point,
    y: Point,
}

unsafe fn sample_blades(actor: u32) -> Option<[Blade; 2]> {
    unsafe {
        let mut blades = [Blade::default(); 2];
        for (blade, joint) in blades.iter_mut().zip(super::BLADE_JOINTS) {
            let matrix = super::joint_matrix(actor, joint)?;
            let m: [f32; 16] = read(matrix, 0);
            *blade = Blade {
                base: core::array::from_fn(|i| m[12 + i] + 15. * m[8 + i]),
                x: [m[0], m[1], m[2]],
                y: [m[4], m[5], m[6]],
            };
        }
        Some(blades)
    }
}

#[derive(Clone, Copy, Default)]
struct Poses {
    hands: [[Blade; 6]; 2],
    active: [bool; 3], // sharpening, right demon blade, left demon blade
}

#[derive(Default)]
pub(super) struct History {
    poses: Poses,
    area: Option<u8>,
    fraction: f32,
    sharpen_time: f32,
    sharpen_motion: u16,
}

// Effect generation runs with the hunter. Draw submission can run on a
// rendering worker. Never keep a hunter or animation pointer in the packet.
struct Shared<T>(AtomicBool, UnsafeCell<T>);
unsafe impl<T: Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    const fn new(value: T) -> Self {
        Self(AtomicBool::new(false), UnsafeCell::new(value))
    }
    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self
            .0
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let result = f(unsafe { &mut *self.1.get() });
        self.0.store(false, Ordering::Release);
        result
    }
}
const EMPTY: Poses = Poses {
    hands: [[Blade {
        base: [0.; 3],
        x: [0.; 3],
        y: [0.; 3],
    }; 6]; 2],
    active: [false; 3],
};
static POSES: Shared<(u32, Poses)> = Shared::new((0, EMPTY));

fn publish(poses: &Poses) {
    POSES.with(|(version, state)| {
        *version = version.wrapping_add(1);
        *state = *poses;
    });
}

impl History {
    pub(super) unsafe fn sharpen(&mut self, actor: u32, phase: u8, ticks: u16) {
        self.sharpen_time = 2. * f32::from(ticks);
        self.sharpen_motion = unsafe { read(actor, 1204) };
        unsafe {
            self.sample(actor, phase, 0.);
        }
    }

    pub(super) unsafe fn sample(&mut self, actor: u32, phase: u8, dt: f32) {
        unsafe {
            if read::<u16>(actor, 1204) != self.sharpen_motion {
                self.sharpen_time = 0.;
            }
            self.sharpen_time = (self.sharpen_time - dt).max(0.);
            let sharpening = self.sharpen_time > 0.;
            let demon = phase != 0;
            let any_active = sharpening || demon;
            if !any_active && self.area.is_none() {
                return;
            }
            let area = read(actor, 4180);
            let blades = if any_active {
                sample_blades(actor)
            } else {
                None
            };
            if let Some(blades) = blades {
                let reset = self.area != Some(area) || dt > 8.;
                self.fraction += dt;
                let advance = self.fraction >= 2.;
                if advance {
                    self.fraction %= 2.;
                }
                for (past, blade) in self.poses.hands.iter_mut().zip(blades) {
                    if reset {
                        past.fill(blade);
                    } else if advance {
                        past.copy_within(0..5, 1);
                        past[0] = blade;
                    }
                }
                self.area = Some(area);
                self.poses.active = [sharpening, demon, demon];
            } else {
                self.poses.active = [false; 3];
                self.area = None;
                self.fraction = 0.;
            }
            publish(&self.poses);
        }
    }
}

impl Drop for History {
    fn drop(&mut self) {
        publish(&EMPTY);
    }
}

const VERTICES: usize = 48;
const STRIDE: usize = 24;
const SIZE: usize = VERTICES * STRIDE;

// Only draws of the same private rModel share its buffer binding. GU stamps
// submitted buffers, so a second camera can read an identical strip in flight.
struct StripBuffers {
    slots: [u32; 12],
    last: Option<CachedStrip>,
}
struct CachedStrip {
    model: u32,
    pose_version: u32,
    origin: Point,
    buffer: u32,
}
static BUFFERS: [Shared<StripBuffers>; 3] = [const {
    Shared::new(StripBuffers {
        slots: [0; 12],
        last: None,
    })
}; 3];

unsafe fn skin(model: u32, origin: Point, poses: &[Blade; 6], buffers: &mut [u32]) -> Option<u32> {
    unsafe {
        let base = forge::mem::text_addr();
        let source = read::<u32>(model, 164);
        if source == 0 || read::<u32>(source, 20) != SIZE as u32 {
            return None;
        }
        let template = read::<u32>(source, 24);
        if template == 0 {
            return None;
        }
        let retired = read::<i32>(base, 0x2124120);
        let slot = buffers
            .iter_mut()
            .find(|b| **b == 0 || read::<i32>(**b, 4) < retired)?;
        if *slot == 0 {
            let allocation = native!(0xb0b26c, 120, 16);
            if allocation == 0 {
                return None;
            }
            *slot = native!(0xb0b2a8, allocation, SIZE as u32, 0, 0);
        }
        let dest = read::<u32>(*slot, 24);
        if dest == 0 {
            return None;
        }
        core::ptr::copy_nonoverlapping(template as *const u8, dest as *mut u8, SIZE);
        // 81F8B4F8 builds 24 matrices, six quadratic samples per span.
        // The FMOD strip stores bone 25 first and bone 2 last.
        for pair in 0..VERTICES / 2 {
            let row = VERTICES / 2 - 1 - pair;
            let span = row / 6;
            let t = (row % 6) as f32 / 6.;
            let u = 1. - t;
            let (a, b, c) = (&poses[span], &poses[span + 1], &poses[span + 2]);
            let base: Point = core::array::from_fn(|i| {
                u * u * (a.base[i] + b.base[i]) * 0.5
                    + 2. * t * u * b.base[i]
                    + t * t * (b.base[i] + c.base[i]) * 0.5
                    - origin[i]
            });
            let x: Point = core::array::from_fn(|i| u * a.x[i] + t * b.x[i]);
            let y: Point = core::array::from_fn(|i| u * a.y[i] + t * b.y[i]);
            let z = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            let length = libm::sqrtf(z.iter().map(|v| v * v).sum());
            let inverse = if length > 0. { 1. / length } else { 0. };
            for vertex in 2 * pair..2 * pair + 2 {
                let offset = (vertex * STRIDE) as u32;
                let distance = read::<f32>(template, offset + 8) * inverse;
                let point: Point = core::array::from_fn(|i| base[i] + z[i] * distance);
                write(dest, offset, point);
            }
        }
        Some(*slot)
    }
}

// Inside A956D8's type-5 branch, after the registry has isolated the
// render state. Keep the resource's original buffer for all later draws.
#[forge::hook(offset = 0xa956f4)]
unsafe fn draw(generator: u32, ctx: u32, gain: u32) -> u32 {
    unsafe {
        let params = read::<u32>(generator, 52);
        if params == 0 || read::<[u8; 22]>(params, 80) != *b"effect\\frontier_db\\m50" {
            return original!(generator, ctx, gain);
        }
        let channel = match read::<[u8; 2]>(params, 102) {
            [b'1', 0] => 0,
            [b'2', 0] => 1,
            [b'3', 0] => 2,
            _ => return original!(generator, ctx, gain),
        };
        let Some((version, poses)) = POSES.with(|(version, s)| {
            s.active[channel].then_some((*version, s.hands[usize::from(channel == 2)]))
        }) else {
            return 0;
        };
        let descriptor = read::<u32>(generator, 40);
        let joint = read::<u32>(generator, 24);
        if descriptor == 0 || joint == 0 {
            return 0;
        }
        let model = read::<u32>(descriptor, 24);
        if model == 0 {
            return 0;
        }
        // A956F4 synchronously submits this model; it does not draw other
        // generators. Serialize the temporary binding only for this resource.
        BUFFERS[channel].with(|buffers| {
            let old = read::<u32>(model, 164);
            let origin = read(joint, 48);
            let buffer = match &buffers.last {
                Some(strip)
                    if (strip.model, strip.pose_version, strip.origin)
                        == (model, version, origin) =>
                {
                    strip.buffer
                }
                _ => {
                    let Some(buffer) = skin(model, origin, &poses, &mut buffers.slots) else {
                        return 0;
                    };
                    buffers.last = Some(CachedStrip {
                        model,
                        pose_version: version,
                        origin,
                        buffer,
                    });
                    buffer
                }
            };
            write(model, 164, buffer);
            let result = original!(generator, ctx, gain);
            write(model, 164, old);
            result
        })
    }
}

pub(crate) fn install() {
    forge::install_hook!(forge::mem::text_addr(), draw);
}
