//! GU's directed-shell setup and initial trajectory. No species or move policy lives here.
use super::MonsterUpdate;
use crate::combat::shell::ShellLaunch;
use crate::memory::{Field, Object, entry, singleton};

/// A directed shell for one monster update: either a native numbered slot or
/// an owning reference to an independent ARC resource.
pub struct DirectedShellResource<'a> {
    owner: MonsterUpdate<'a>,
    binding: Binding,
}
enum Binding {
    Number(u32),
    Private(u32),
}

impl Drop for DirectedShellResource<'_> {
    fn drop(&mut self) {
        if let Binding::Private(resource) = self.binding {
            unsafe {
                let release: unsafe extern "C" fn(u32) = core::mem::transmute(entry(0x887660));
                release(resource);
            }
        }
    }
}

impl<'a> DirectedShellResource<'a> {
    /// Wrap a resource ID obtained from the current update's native bindings.
    /// Out-of-range IDs (including the unassigned sentinel) return None.
    ///
    /// # Safety
    /// For an in-range ID the binding must establish that this owner's resource
    /// accepts setup 3F93F8 (variant zero) and produces the 3F97EC trajectory layout.
    /// The numeric range alone does not establish the shell class.
    pub unsafe fn from_native_id(owner: MonsterUpdate<'a>, id: u32) -> Option<Self> {
        // 48C4F0 indexes a 412-entry table.
        (id < 412).then_some(Self {
            owner,
            binding: Binding::Number(id),
        })
    }

    /// Resolve an owning rShell resource from an ARC by its GU path hash.
    /// The resource must describe a GU directed shell, independent of retail numbered slots.
    /// # Safety
    /// The caller must supply an asset authored for the directed shell class.
    pub unsafe fn from_asset(owner: MonsterUpdate<'a>, path_hash: u32) -> Option<Self> {
        unsafe {
            let manager = singleton(0x2126a64)?;
            let lookup = method(manager, 56);
            let find: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
                core::mem::transmute(lookup);
            let resource = find(manager.addr(), lookup, path_hash, 0x4aa69872);
            (resource != 0).then(|| Self {
                owner,
                binding: Binding::Private(resource),
            })
        }
    }

    /// Spawn with the resolved resource and owner. Returns false if GU refuses or
    /// the trajectory is invalid. No native setup or instance pointer escapes.
    pub fn spawn(&self, launch: ShellLaunch) -> bool {
        if !launch
            .origin
            .iter()
            .chain(&launch.velocity)
            .all(|v| v.is_finite())
        {
            return false;
        }
        unsafe {
            let Some(manager) = singleton(0x1892550) else {
                return false;
            };
            let shell = match self.binding {
                Binding::Number(id) => {
                    let Some(setup) = self.setup(&launch, Some(id)) else {
                        return false;
                    };
                    // Registration takes ownership of setup, even on rejection.
                    let register: unsafe extern "C" fn(u32, u32, u32) -> u32 =
                        core::mem::transmute(entry(0x48c4f0));
                    Object::at(register(manager.addr(), setup.addr(), 0))
                }
                Binding::Private(resource) => self.spawn_private(manager, resource, &launch),
            };
            let Some(shell) = shell else { return false };
            shell.set(POSITION, launch.origin);
            shell.set(TRAJECTORY_ORIGIN, launch.origin);
            shell.set(VELOCITY, launch.velocity);
            shell.set(PITCH, u32::from(launch.pitch.units()));
            shell.set(YAW, u32::from(launch.yaw.units()));
            true
        }
    }

    unsafe fn setup(&self, launch: &ShellLaunch, number: Option<u32>) -> Option<Object<'a>> {
        unsafe {
            let construct: unsafe extern "C" fn() -> u32 = core::mem::transmute(entry(0x3f93f8));
            let setup = Object::at(construct())?;
            // For an independent asset retain the constructor's unnumbered sentinel (413).
            if let Some(id) = number {
                setup.set(SETUP_RESOURCE, id);
            }
            setup.set(SETUP_VARIANT, 0);
            setup.set(SETUP_OWNER, self.owner.monster().as_raw());
            setup.set(SETUP_ORIGIN, launch.origin);
            setup.set(SETUP_PITCH, u32::from(launch.pitch.units()));
            setup.set(SETUP_YAW, u32::from(launch.yaw.units()));
            Some(setup)
        }
    }

    unsafe fn spawn_private(
        &self,
        manager: Object<'a>,
        resource: u32,
        launch: &ShellLaunch,
    ) -> Option<Object<'a>> {
        let owner_state = self.owner.monster().0.get(Field::<u32>::at(12)) & 7;
        if !matches!(owner_state, 1 | 2) {
            return None;
        }
        // GU's enlist routine inserts at the first vacant shell slot.
        if !(0..144).any(|i| manager.get(Field::<u32>::at(28044 + 4 * i)) == 0) {
            return None;
        }

        unsafe {
            let scene = singleton(0x2126f48)?;
            let factory = Object::at(entry(0x1892de8) as u32)?;
            let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(method(factory, 8));
            let shell = Object::at(create(factory.addr()))?;
            let bind: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(entry(0x4a1c84));
            bind(shell.addr(), resource);

            let Some(setup) = self.setup(launch, None) else {
                let cancel: unsafe extern "C" fn(u32) = core::mem::transmute(method(shell, 64));
                cancel(shell.addr());
                return None;
            };
            let init: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(method(shell, 316));
            if init(shell.addr(), setup.addr()) != 1 {
                return None;
            }
            let attach: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) =
                core::mem::transmute(entry(0xc067ec));
            attach(scene.addr(), 0x12, shell.addr(), 0, 0, 0);
            let enlist: unsafe extern "C" fn(u32, u32) = core::mem::transmute(entry(0x48c6c8));
            enlist(manager.addr(), shell.addr());
            Some(shell)
        }
    }
}

unsafe fn method(object: Object<'_>, offset: u32) -> u32 {
    let vtable = object.get(Field::<u32>::at(0));
    unsafe { ((vtable + offset) as *const u32).read() }
}

const SETUP_RESOURCE: Field<u32> = Field::at(4);
const SETUP_VARIANT: Field<u32> = Field::at(8);
const SETUP_OWNER: Field<u32> = Field::at(12);
const SETUP_ORIGIN: Field<[f32; 3]> = Field::at(16);
const SETUP_PITCH: Field<u32> = Field::at(32);
const SETUP_YAW: Field<u32> = Field::at(36);
const POSITION: Field<[f32; 3]> = Field::at(64);
const PITCH: Field<u32> = Field::at(4072);
const YAW: Field<u32> = Field::at(4076);
const TRAJECTORY_ORIGIN: Field<[f32; 3]> = Field::at(4096);
const VELOCITY: Field<[f32; 3]> = Field::at(4112);
