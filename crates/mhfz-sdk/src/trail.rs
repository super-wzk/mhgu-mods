//! Skin a Frontier blade strip from six world-space weapon poses.
use crate::sync::Shared;
use core::ffi::CStr;
use mhgu_sdk::service;

unsafe fn entry(offset: u32) -> usize {
    (unsafe { forge_sys::mem::forge_mem_getMainTextAddr() } + offset) as usize
}

unsafe fn read<T: Copy>(p: u32, at: u32) -> T {
    unsafe { ((p + at) as *const T).read_unaligned() }
}
unsafe fn write<T>(p: u32, at: u32, value: T) {
    unsafe { ((p + at) as *mut T).write_unaligned(value) }
}
type Point = [f32; 3];

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Blade {
    pub base: Point,
    pub x: Point,
    pub y: Point,
    pub length: f32,
}

impl Blade {
    pub fn from_pose(m: &[f32; 16], inset: f32, tip_z: f32) -> Option<Self> {
        let [x, y, z, origin] = *m.as_chunks::<4>().0.first_chunk::<4>()?;
        let start_z = inset.min(tip_z * 0.5);
        let axis_scale = libm::sqrtf(z[0] * z[0] + z[1] * z[1] + z[2] * z[2]);
        let length = (tip_z - start_z) * axis_scale;
        (length.is_finite() && length > 0.).then(|| Self {
            base: core::array::from_fn(|i| origin[i] + start_z * z[i]),
            x: [x[0], x[1], x[2]],
            y: [y[0], y[1], y[2]],
            length,
        })
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
pub struct Renderer(Shared<StripBuffers>);
impl Renderer {
    pub const fn new() -> Self {
        Self(Shared::new(StripBuffers {
            slots: [0; 12],
            last: None,
        }))
    }
}
unsafe fn skin(
    model: u32,
    origin: Point,
    poses: &[Blade; 6],
    tint: [u8; 4],
    buffers: &mut [u32],
) -> Option<u32> {
    unsafe {
        let base = entry(0) as u32;
        let source = read::<u32>(model, 164);
        if source == 0 || read::<u32>(source, 20) != SIZE as u32 {
            return None;
        }
        let template = read::<u32>(source, 24);
        if template == 0 {
            return None;
        }
        // Each converted strip starts with the outer vertex of its first pair.
        let authored_span = read::<f32>(template, 8);
        if !authored_span.is_finite() || authored_span <= 0. {
            return None;
        }
        let blade_scale = poses[0].length / authored_span;
        let retired = read::<i32>(base, 0x2124120);
        let slot = buffers
            .iter_mut()
            .find(|b| **b == 0 || read::<i32>(**b, 4) < retired)?;
        if *slot == 0 {
            let alloc: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(entry(0xb0b26c));
            let allocation = alloc(120, 16);
            if allocation == 0 {
                return None;
            }
            let init: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
                core::mem::transmute(entry(0xb0b2a8));
            *slot = init(allocation, SIZE as u32, 0, 0);
            if *slot == 0 {
                return None;
            }
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
            let axis_length = libm::sqrtf(z.iter().map(|v| v * v).sum());
            let distance_scale = if axis_length > 0. {
                blade_scale / axis_length
            } else {
                0.
            };
            for vertex in 2 * pair..2 * pair + 2 {
                let offset = (vertex * STRIDE) as u32;
                let distance = read::<f32>(template, offset + 8) * distance_scale;
                let point: Point = core::array::from_fn(|i| base[i] + z[i] * distance);
                write(dest, offset, point);
                let rgba = read::<[u8; 4]>(template, offset + 20);
                write(
                    dest,
                    offset + 20,
                    core::array::from_fn::<_, 4, _>(|i| {
                        (u16::from(rgba[i]) * u16::from(tint[i]) / 255) as u8
                    }),
                );
            }
        }
        Some(*slot)
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Sample {
    pub poses: [Blade; 6],
    pub tint: [u8; 4],
}
pub type Snapshot = unsafe extern "C" fn(*mut Sample) -> u32;
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(*const core::ffi::c_char, Snapshot));
const NAME: &CStr = c"mhgu::StyleBladeTrailV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SBT1");
/// # Safety
/// The registry publishes this process-lifetime service during Forge initialization.
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}
/// # Safety
/// Both arguments must remain valid on the render thread until process exit.
pub unsafe fn bind(path: &'static CStr, sample: Snapshot) {
    if service::on_game_thread()
        && let Some(api) = (unsafe { service::find::<Service>(NAME, MAGIC) })
    {
        unsafe {
            (api.0)(path.as_ptr(), sample);
        }
    }
}

impl Renderer {
    /// # Safety
    /// Requires a live type-5 generator for this strip and its synchronous GU draw.
    pub unsafe fn draw(
        &self,
        generator: u32,
        version: u32,
        sample: &Sample,
        draw: impl FnOnce() -> u32,
    ) -> u32 {
        unsafe {
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
            self.0.with(|buffers| {
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
                        let Some(buffer) = skin(
                            model,
                            origin,
                            &sample.poses,
                            sample.tint,
                            &mut buffers.slots,
                        ) else {
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
                let result = draw();
                write(model, 164, old);
                result
            })
        }
    }
}
