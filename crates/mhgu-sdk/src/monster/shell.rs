//! GU's directed-shell setup and initial trajectory. No species or move policy lives here.
use super::MonsterUpdate;
use crate::combat::shell::ShellLaunch;
use crate::memory::{Field, Object, entry, singleton};

/// A directed-shell resource resolved for one monster update. GU uses a table ID,
/// not a pointer to an asset or a byte offset in the actor. This handle does not own it.
pub struct DirectedShellResource<'a> {
    owner: MonsterUpdate<'a>,
    id: u32,
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
        (id < 412).then_some(Self { owner, id })
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
            let construct: unsafe extern "C" fn() -> u32 = core::mem::transmute(entry(0x3f93f8));
            let Some(setup) = Object::at(construct()) else {
                return false;
            };
            setup.set(SETUP_RESOURCE, self.id);
            setup.set(SETUP_VARIANT, 0);
            setup.set(SETUP_OWNER, self.owner.monster().as_raw());
            setup.set(SETUP_ORIGIN, launch.origin);
            setup.set(SETUP_PITCH, u32::from(launch.pitch.units()));
            setup.set(SETUP_YAW, u32::from(launch.yaw.units()));
            // Ownership transfers on the call, including rejection. No intervening early return.
            let register: unsafe extern "C" fn(u32, u32, u32) -> u32 =
                core::mem::transmute(entry(0x48c4f0));
            let Some(shell) = Object::at(register(manager.addr(), setup.addr(), 0)) else {
                return false;
            };
            shell.set(POSITION, launch.origin);
            shell.set(TRAJECTORY_ORIGIN, launch.origin);
            shell.set(VELOCITY, launch.velocity);
            shell.set(PITCH, u32::from(launch.pitch.units()));
            shell.set(YAW, u32::from(launch.yaw.units()));
            true
        }
    }
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
