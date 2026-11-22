//! GU particle resources, attachment proofs and floor sampling for hunter styles.
use super::resource::CachedResource;
use crate::{
    memory::{Field, Object, entry, singleton},
    service,
};
use core::{
    ffi::CStr,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

const RELEASE_SERVICE: &CStr = c"mhgu::ParticleReleaseV1";
const RELEASE_MAGIC: u32 = u32::from_le_bytes(*b"PRL1");
pub type OnRelease = extern "C" fn(u32);

#[doc(hidden)]
#[repr(C)]
pub struct ReleaseService(pub extern "C" fn(u32, OnRelease) -> bool);

/// # Safety
/// Publish once from the registry during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_release(api: &'static ReleaseService) -> bool {
    unsafe { service::publish(RELEASE_SERVICE, RELEASE_MAGIC, api) }
}

/// Subscribe to particle releases before retaining any native unit pointers.
pub fn on_release(style: u32, released: OnRelease) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<ReleaseService>(RELEASE_SERVICE, RELEASE_MAGIC) }
            .is_some_and(|api| (api.0)(style, released))
}

const MODEL_SERVICE: &CStr = c"mhgu::ParticleModelV1";
const MODEL_MAGIC: u32 = u32::from_le_bytes(*b"PMD1");

/// GU 1.4's synchronous type-5 model draw.
pub type DrawModel = unsafe extern "C" fn(generator: u32, context: u32, gain: u32) -> u32;

#[repr(C)]
pub struct ModelRenderer {
    /// Select this renderer from the live particle definition.
    pub matches: unsafe extern "C" fn(params: u32) -> bool,
    /// Draw synchronously. The native continuation borrows the same GU objects.
    pub draw: unsafe extern "C" fn(
        DrawModel,
        generator: u32,
        params: u32,
        context: u32,
        gain: u32,
    ) -> u32,
}

#[doc(hidden)]
#[repr(C)]
pub struct ModelService(pub unsafe extern "C" fn(*const ModelRenderer));

/// # Safety
/// Publish once from the registry on the Forge init thread.
#[doc(hidden)]
pub unsafe fn publish_model(api: &'static ModelService) -> bool {
    unsafe { service::publish(MODEL_SERVICE, MODEL_MAGIC, api) }
}

/// Attach a process-lifetime renderer for a set of private GU models.
pub fn bind_model(renderer: &'static ModelRenderer) -> bool {
    if !service::on_game_thread() {
        return false;
    }
    let Some(api) = (unsafe { service::find::<ModelService>(MODEL_SERVICE, MODEL_MAGIC) }) else {
        return false;
    };
    unsafe { (api.0)(renderer) };
    true
}

/// End a live, release-tracked unit on GU's particle thread.
///
/// # Safety
/// The caller must remove the handle on native release and before stopping it.
pub unsafe fn stop_unit(unit: u32) {
    let stop: unsafe extern "C" fn(u32, u32) = unsafe { core::mem::transmute(entry(0x32a400)) };
    unsafe { stop(unit, 1) };
}

/// Tracks particle units owned by one action, including units waiting to emit.
pub struct TrackedUnits<const N: usize> {
    units: [AtomicU32; N],
    ready: AtomicBool,
}

impl<const N: usize> TrackedUnits<N> {
    pub const fn new() -> Self {
        Self {
            units: [const { AtomicU32::new(0) }; N],
            ready: AtomicBool::new(false),
        }
    }

    pub fn bind(&'static self, style: u32, released: OnRelease) -> bool {
        let ready = on_release(style, released);
        self.ready.store(ready, Ordering::Release);
        ready
    }

    pub fn forget(&self, unit: u32) {
        for slot in &self.units {
            let _ = slot.compare_exchange(unit, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }

    /// # Safety
    /// Call on the particle system's game thread.
    pub unsafe fn stop(&self) {
        unsafe { self.stop_slots(0..N) };
    }

    /// Stop the units tracked for one owner, including pending emissions.
    /// # Safety
    /// Call on the particle system's game thread.
    pub unsafe fn stop_slots(&self, range: core::ops::Range<usize>) {
        for slot in &self.units[range] {
            let unit = slot.swap(0, Ordering::AcqRel);
            if unit != 0 {
                // End live children and cancel units still waiting to emit.
                unsafe { stop_unit(unit) };
            }
        }
    }

    /// # Safety
    /// Call on the game thread with a live anchor. Retry only the mask
    /// returned here so an accepted unit always keeps its tracking slot.
    pub unsafe fn emit_layers(
        &self,
        bank: &Bank,
        route: usize,
        anchor: Anchor,
        pending: u32,
        first_slot: usize,
    ) -> u32 {
        let Some(count) = bank.layer_count(route) else {
            return 0;
        };
        if !self.ready.load(Ordering::Acquire) || first_slot + usize::from(count) > N {
            return 0;
        }
        unsafe {
            bank.emit_layers(route, anchor, pending, |layer, unit| {
                self.units[first_slot + layer].store(unit, Ordering::Release);
            })
        }
    }
}

#[derive(Clone, Copy)]
pub enum Anchor {
    World([f32; 3], u16),
    RotatedWorld([f32; 3], [u16; 3]),
    /// World pose with Ry * Rx * Rz rotation and uniform scale.
    ScaledWorld {
        position: [f32; 3],
        angles: [u16; 3],
        scale: f32,
    },
    Joint {
        parent: u32,
        joint: i16,
        offset: Option<[f32; 3]>,
    },
    /// Follow the live joint basis even when the PEL defaults to world orientation.
    JointPose {
        parent: u32,
        joint: i16,
        offset: Option<[f32; 3]>,
    },
    /// Follow the live joint position with the supplied world yaw.
    JointYaw {
        parent: u32,
        joint: i16,
        offset: Option<[f32; 3]>,
        yaw: u16,
    },
}

/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn world_offset(actor: u32, [x, y, z]: [f32; 3]) -> ([f32; 3], u16) {
    unsafe {
        let [px, py, pz] = ((actor + 64) as *const [f32; 3]).read_unaligned();
        let yaw = ((actor + 4076) as *const u16).read_unaligned();
        let (sin, cos) = crate::geometry::Angle::from_units(yaw).sin_cos();
        (
            [px + cos * x + sin * z, py + y, pz - sin * x + cos * z],
            yaw,
        )
    }
}

pub fn transform_offset(m: &[f32; 16], offset: [f32; 3]) -> [f32; 3] {
    core::array::from_fn(|i| {
        m[12 + i] + m[i] * offset[0] + m[4 + i] * offset[1] + m[8 + i] * offset[2]
    })
}

pub struct Bank {
    routes: &'static [u8],
    resource: CachedResource,
}

impl Bank {
    pub const fn new(path: &'static CStr, routes: &'static [u8]) -> Self {
        Self {
            routes,
            resource: CachedResource::new(0x1889fdc, path),
        }
    }

    /// Queue the PEL and its dependencies on GU's resource loader.
    ///
    /// # Safety
    /// Call on the game thread after the native resource manager is initialized.
    pub unsafe fn preload(&self) {
        unsafe { self.resource.preload() };
    }

    /// Borrow the process-lifetime PEL resource for custom particle units.
    ///
    /// # Safety
    /// Call on the game thread after the native resource manager is initialized.
    pub unsafe fn resource(&self) -> u32 {
        unsafe { self.resource.get() }.unwrap_or(0)
    }

    /// Spawn the selectors in this route using a world or joint anchor.
    ///
    /// # Safety
    /// Call on the game thread. A joint anchor must have a live particle parent.
    pub unsafe fn emit(&self, route: usize, anchor: Anchor) {
        unsafe { self.emit_tracked(route, anchor, |_| {}) };
    }

    /// Report every accepted unit, including routes with more than 32 layers.
    ///
    /// # Safety
    /// The same game thread and attachment requirements as `emit` apply.
    pub unsafe fn emit_tracked(&self, route: usize, anchor: Anchor, mut accepted: impl FnMut(u32)) {
        let Some((first, count)) = self.route(route) else {
            return;
        };
        unsafe { self.emit_selectors(anchor, first..first + count, |_, unit| accepted(unit)) };
    }

    fn route(&self, route: usize) -> Option<(u16, u16)> {
        let &[lo, hi, count_lo, count_hi] = self.routes.as_chunks::<4>().0.get(route)?;
        Some((
            u16::from_le_bytes([lo, hi]),
            u16::from_le_bytes([count_lo, count_hi]),
        ))
    }

    pub fn layer_count(&self, route: usize) -> Option<u16> {
        self.route(route).map(|(_, count)| count)
    }

    /// Spawn pending layers for a route with at most 32 layers. The returned
    /// mask contains only layers awaiting a successful submission.
    ///
    /// # Safety
    /// Call on the game thread with a live anchor.
    pub unsafe fn emit_layers(
        &self,
        route: usize,
        anchor: Anchor,
        pending: u32,
        mut accepted: impl FnMut(usize, u32),
    ) -> u32 {
        let Some((first, count)) = self.route(route) else {
            return 0;
        };
        if count == 0 || count > 32 {
            return 0;
        }
        let mut remaining = pending & (u32::MAX >> (32 - count));
        if remaining == 0 {
            return 0;
        }
        let requested = (0..count)
            .filter(|&i| pending & (1 << i) != 0)
            .map(|i| first + i);
        unsafe {
            self.emit_selectors(anchor, requested, |selector, unit| {
                let layer = usize::from(selector - first);
                remaining &= !(1 << layer);
                accepted(layer, unit);
            });
        }
        remaining
    }

    unsafe fn emit_selectors(
        &self,
        anchor: Anchor,
        selectors: impl Iterator<Item = u16>,
        mut accepted: impl FnMut(u16, u32),
    ) {
        unsafe {
            let Some(manager) = singleton(0x188a0d4) else {
                return;
            };
            if singleton(0x2126f48).is_none() {
                return;
            }
            let pel = self.resource();
            if pel == 0 {
                return;
            }
            let acquire: unsafe extern "C" fn(u32, u32, u32) -> u32 =
                core::mem::transmute(entry(0x3268d8));
            let Some(handle) = Object::at(acquire(manager.addr(), pel, 0)) else {
                return;
            };
            let id = handle.get(Field::<u32>::at(4));
            if id == u32::MAX {
                return;
            }
            let allocate: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(entry(0x3288c4));
            let init: unsafe extern "C" fn(u32) = core::mem::transmute(entry(0x328900));
            let spawn: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) -> u32 =
                core::mem::transmute(entry(0x3267d0));
            for selector in selectors {
                let Some(unit) = Object::at(allocate(416, 16)) else {
                    continue;
                };
                init(unit.addr());
                let proof = unit.embedded(128);
                let world = |position: [f32; 3], angles: [u16; 3]| {
                    proof.set(Field::at(4), 0xau32);
                    proof.set(Field::at(12), 1u32);
                    proof.set(Field::at(70), 3u8);
                    proof.set(Field::at(176), position);
                    proof.set(
                        Field::at(32),
                        angles.map(|angle| f32::from(angle) * 360. / 65536.),
                    );
                };
                match anchor {
                    Anchor::World(position, yaw) => world(position, [0, yaw, 0]),
                    Anchor::RotatedWorld(position, angles) => world(position, angles),
                    Anchor::ScaledWorld {
                        position,
                        angles,
                        scale,
                    } => {
                        world(position, angles);
                        // Override the PEL Euler order for 32AA44 and 327608.
                        // Property 30 makes 31C3A8 multiply the scales.
                        proof.set(Field::at(4), 0xau32 | (1 << 4) | (1 << 30));
                        proof.set(Field::at(71), 4u8);
                        proof.set(Field::at(48), [scale; 3]);
                    }
                    Anchor::Joint {
                        parent,
                        joint,
                        offset,
                    }
                    | Anchor::JointPose {
                        parent,
                        joint,
                        offset,
                    }
                    | Anchor::JointYaw {
                        parent,
                        joint,
                        offset,
                        ..
                    } => {
                        if let Some(offset) = offset {
                            proof.set(Field::at(16), offset);
                        }
                        let mut properties = 0x28u32 | u32::from(offset.is_some());
                        match anchor {
                            Anchor::JointPose { .. } => {
                                // GU 31CC6C keeps explicitly supplied PEL property 7.
                                properties |= 1 << 7;
                                proof.set(Field::at(72), 0u8);
                            }
                            Anchor::JointYaw { yaw, .. } => {
                                properties |= (1 << 1) | (1 << 7);
                                proof.set(Field::at(32), [0., f32::from(yaw) * 360. / 65536., 0.]);
                                proof.set(Field::at(72), 2u8);
                            }
                            _ => {}
                        }
                        proof.set(Field::at(4), properties);
                        proof.set(Field::at(12), 2u32);
                        proof.set(Field::at(66), joint);
                        proof.set(Field::at(70), 0u8);
                        proof.set(Field::at(192), parent);
                    }
                }
                if spawn(manager.addr(), unit.addr(), id, 1, u32::from(selector), 0) == 0 {
                    let vtable: *const usize = unit.get(Field::at(0));
                    let destroy: unsafe extern "C" fn(u32) =
                        core::mem::transmute(vtable.add(1).read());
                    destroy(unit.addr());
                } else {
                    accepted(selector, unit.addr());
                }
            }
            let release: unsafe extern "C" fn(u32, u32, u32) =
                core::mem::transmute(entry(0x326c78));
            release(manager.addr(), handle.addr(), 0);
        }
    }
}

/// # Safety
/// The actor must be a live GU hunter with its model initialized.
pub unsafe fn parent(actor: u32) -> Option<u32> {
    unsafe {
        let actor = Object::at(actor)?;
        if actor.get(Field::<u32>::at(1172)) == 0 || actor.get(Field::<u32>::at(1176)) == 0 {
            return None;
        }
        let vtable: *const usize = actor.get(Field::at(0));
        let get: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(vtable.add(304 / 4).read());
        let parent = get(actor.addr());
        (parent != 0).then_some(parent)
    }
}

/// Draw from GU's particle RNG (AECE50), independently of combat rolls.
/// # Safety
/// Call only on the game's particle thread.
pub unsafe fn random_u32() -> u32 {
    unsafe {
        let Some(manager) = singleton(0x2126554) else { return 0 };
        let draw: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(entry(0xb91e68));
        draw(manager.addr(), 1)
    }
}

/// # Safety
/// The actor must be a live GU hunter. The returned matrix is borrowed for the call.
pub unsafe fn joint_matrix(actor: u32, joint: u32) -> Option<u32> {
    unsafe {
        let model = Object::at(actor)?;
        if model.get(Field::<u32>::at(1172)) == 0 || model.get(Field::<u32>::at(1176)) == 0 {
            return None;
        }
        let get: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(entry(0xc1628));
        let matrix = get(actor, joint);
        // GU returns the actor root for an unmapped joint.
        (matrix != 0 && matrix != actor + 176).then_some(matrix)
    }
}

/// Query the floor and highest special surface at the supplied world position.
///
/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn ground_heights(actor: u32, position: [f32; 3]) -> Option<(f32, Option<f32>)> {
    unsafe {
        let manager = singleton(0x21270e0)?;
        let actor = Object::at(actor)?;
        let mut hit = [0u32; 12];
        let pos = [position[0], position[1], position[2], 0.];
        let mut floor = 0f32;
        let query: unsafe extern "C" fn(
            u32,
            u32,
            *const f32,
            *const f32,
            u32,
            *mut f32,
            *mut u32,
            u32,
            u32,
            u32,
        ) -> u32 = core::mem::transmute(entry(0x18192c));
        if query(
            manager.addr(),
            actor.addr(),
            pos.as_ptr(),
            pos.as_ptr(),
            u32::from(actor.get(Field::<u8>::at(4180))),
            &mut floor,
            hit.as_mut_ptr(),
            0,
            0,
            0,
        ) != 1
        {
            return None;
        }
        let surface = f32::from_bits(hit[5]);
        Some((
            floor,
            (surface > -100000. && surface >= floor - 1.).then_some(surface),
        ))
    }
}
