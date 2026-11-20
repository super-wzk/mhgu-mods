//! Independent style actions on GU's shared hunter state lifecycle.
//!
//! The weapon group remains native (16); the upper half of its slot namespace
//! belongs to the selected custom style, rather than to a stock weapon handler.

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod attack;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use attack::Attack;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod motion_bank;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use motion_bank::MotionBanks;
pub mod command;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod melee;

pub const GROUP: u16 = 16;
pub const FIRST: u16 = 0x8000;

pub const fn slot(action: u8) -> u16 {
    FIRST + action as u16
}

pub const fn custom(group: u16, slot: u16) -> bool {
    group == GROUP && slot >= FIRST && slot < FIRST + 256
}

pub const fn custom_id(group: u16, slot: u16) -> Option<u8> {
    if custom(group, slot) {
        Some((slot - FIRST) as u8)
    } else {
        None
    }
}

/// Timing of the hunter's currently playing motion layer.
#[derive(Clone, Copy)]
pub struct MotionFrame {
    pub frame: f32,
    pub end: f32,
    pub restart: f32,
    pub blend: f32,
    pub ended: bool,
}

impl MotionFrame {
    /// # Safety
    /// The actor must be a live GU hunter.
    #[cfg(all(target_arch = "arm", target_endian = "little"))]
    pub unsafe fn sample(actor: u32) -> Self {
        unsafe {
            Self {
                frame: ((actor + 1280) as *const f32).read(),
                end: ((actor + 1284) as *const f32).read(),
                restart: ((actor + 1288) as *const f32).read(),
                blend: ((actor + 1268) as *const f32).read(),
                ended: ((actor + 1206) as *const u8).read() & 4 != 0,
            }
        }
    }

    pub fn complete(self) -> bool {
        self.ended || (self.end > 0. && self.frame >= self.end)
    }
}

/// GU 11681DC tests the active skill and its compound-skill equivalents.
#[derive(Clone, Copy)]
pub enum ChargeSkill {
    Normal,
    Focus,
    Slow,
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
impl ChargeSkill {
    /// # Safety
    /// The actor must be a live hunter on the game thread.
    pub unsafe fn equipped(actor: u32) -> Self {
        let has: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(crate::memory::entry(0x2864c4)) };
        if [152, 266, 284, 293]
            .into_iter()
            .any(|id| unsafe { has(actor, id) } != 0)
        {
            Self::Focus
        } else if unsafe { has(actor, 153) } != 0 {
            Self::Slow
        } else {
            Self::Normal
        }
    }
}

/// Read the hunter's current action group and slot.
///
/// # Safety
/// The actor must be a live GU hunter.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn current(actor: u32) -> (u16, u16) {
    unsafe {
        (
            ((actor + 9478) as *const u16).read(),
            ((actor + 9482) as *const u16).read(),
        )
    }
}

/// Read the previous native action from GU's transition history.
///
/// # Safety
/// The actor must be a live GU hunter.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn previous(actor: u32) -> (u16, u16) {
    unsafe {
        (
            ((actor + 9480) as *const u16).read(),
            ((actor + 9484) as *const u16).read(),
        )
    }
}

// Forge uses the soft-float ABI. GU's layer setter reads s0..s2 and uses the
// actor's motion encoder, so plugged-in motions enter through the same path.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global mhgu_style_motion",
    ".hidden mhgu_style_motion",
    ".type mhgu_style_motion,%function",
    "mhgu_style_motion:",
    "vmov s0,r2",
    "vmov s1,r3",
    "vmov.f32 s2,#1.0",
    "mov r2,r1",
    "mov r1,#0",
    "mov r3,#0",
    "ldr ip,[sp]",
    "bx ip",
    ".global mhgu_style_land",
    ".hidden mhgu_style_land",
    ".type mhgu_style_land,%function",
    "mhgu_style_land:",
    "vmov s0,r1",
    "mov r1,#0",
    "bx r2",
);

/// Run GU's airborne landing check with the same zero tolerance as its dual blades.
///
/// # Safety
/// The actor must be a live airborne hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn land(actor: u32) -> bool {
    unsafe extern "C" {
        fn mhgu_style_land(actor: u32, tolerance: u32, entry: usize) -> u32;
    }
    unsafe { mhgu_style_land(actor, 0, crate::memory::entry(0x2a84d8)) == 1 }
}

/// Play a motion request through GU's registered weapon motion encoder.
///
/// # Safety
/// The actor must be a live hunter with a loaded model on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn play_motion(actor: u32, request: u32, start: f32, blend: f32) -> Option<u16> {
    unsafe extern "C" {
        fn mhgu_style_motion(actor: u32, request: u32, blend: u32, start: u32, entry: usize)
        -> u32;
    }
    unsafe {
        if ((actor + 240) as *const u32).read() == 0 {
            return None;
        }
        let ok = mhgu_style_motion(
            actor,
            request,
            blend.to_bits(),
            start.to_bits(),
            crate::memory::entry(0x53a06c),
        );
        (ok != 0).then(|| ((actor + 1204) as *const u16).read())
    }
}

/// Restore the layer-end signal GU reads at 950BCC after both playheads
/// have stopped at the last frame (951A80). The native action handles its exit.
///
/// # Safety
/// The caller must verify the active action, phase and clip expect this signal.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn signal_stopped_motion(actor: u32) {
    unsafe {
        let flags = (actor + 1206) as *mut u8;
        let end = *((actor + 1284) as *const f32);
        if flags.read() & 4 == 0
            && end > 0.
            && *((actor + 28) as *const f32) > 0.
            && *((actor + 1276) as *const f32) == end
            && *((actor + 1280) as *const f32) == end
        {
            flags.write(flags.read() | 4);
        }
    }
}

/// Weapon action IDs owned by a style. GU dispatches these in its weapon group.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Actions([u32; 8]);

impl Actions {
    pub const fn new(count: u8) -> Self {
        let mut actions = Self([0; 8]);
        let mut id = 0;
        while id < count {
            actions = actions.with(id);
            id += 1;
        }
        actions
    }

    pub const fn with(mut self, id: u8) -> Self {
        self.0[(id / 32) as usize] |= 1 << (id % 32);
        self
    }

    pub const fn contains(self, id: u8) -> bool {
        self.0[(id / 32) as usize] & (1 << (id % 32)) != 0
    }
}

/// Request a style action through GU's common change-state implementation.
/// It resets the native action phase, previous-action history, contact state,
/// interruption flags and motion bookkeeping before the style starts its clip.
///
/// # Safety
/// `actor` must be the selected local hunter during its controller callback.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn enter(actor: u32, action: u8) -> bool {
    unsafe {
        let slot = slot(action);
        let change: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            core::mem::transmute(crate::memory::entry(0x2ab670));
        change(actor, GROUP.into(), slot.into(), 0);
        current(actor) == (GROUP, slot)
    }
}

/// A single GU command-table entry. Frame values use GU's motion clock.
#[derive(Clone, Copy)]
pub struct Transition {
    pub group: u16,
    pub slot: u16,
    pub flags: u16,
    pub condition: u16,
    pub parameter: u32,
    pub start: i16,
    pub end: i16,
}

impl Transition {
    /// A transition whose input and eligibility have already been resolved.
    const fn accepted(group: u16, slot: u16) -> Self {
        Self {
            group,
            slot,
            flags: 0,
            condition: 0,
            parameter: 0,
            start: 0,
            end: command::MAX_COMMAND_FRAME,
        }
    }

    /// The shared GU command which resolves and consumes a pending Hunter Art.
    pub const HUNTER_ART: Self = Self {
        condition: 0x13,
        ..Self::accepted(0x8010, 241)
    };
}

/// Submit a selected branch and confirm that GU entered its destination.
///
/// # Safety
/// The actor and destination must satisfy the requirements of `transition`.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn transition_to(actor: u32, group: u16, slot: u16) -> bool {
    unsafe {
        transition(actor, Transition::accepted(group, slot)) && current(actor) == (group, slot)
    }
}

/// Run GU's Hunter Art branch once the controller opens an activation window.
///
/// # Safety
/// The actor must be the selected local hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn activate_hunter_art(actor: u32) -> bool {
    unsafe {
        // 2A8B34 checks queued slot 0..2 or commands 64..66. 285EE8
        // reads the current command bank at +5172. Preserve custom readers.
        if *((actor + 10068) as *const u32) == 3 && *((actor + 5180) as *const u32) & 7 == 0 {
            let vtable = *(actor as *const *const usize);
            if *vtable.add(744 / 4) == crate::memory::entry(0x285ee8) {
                return false;
            }
        }
        let previous = current(actor);
        transition(actor, Transition::HUNTER_ART) && current(actor) != previous
    }
}

/// Evaluate a style transition with GU's command-table dispatcher. The style
/// supplies movelist-specific eligibility and GU runs the condition and action.
///
/// # Safety
/// `actor` must be the selected local hunter on the game thread. The entry must
/// use valid GU command opcodes, or a registered style action.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn transition(actor: u32, rule: Transition) -> bool {
    use command::native::{CAPACITY, Command, Entry, Resource, Table, buffer, table};
    let resource = Resource {
        command: Command {
            group: rule.group,
            slot: rule.slot,
            flags: rule.flags,
            condition: rule.condition,
            parameters: [rule.parameter, 0, 0, 0],
        },
        ..Resource::EMPTY
    };
    let entry = Entry::new(&resource, 0, rule.start, rule.end);
    let entries = [core::ptr::from_ref(&entry)];
    let commands_for_rule = Table {
        count: 1,
        capacity: 1,
        entries: entries.as_ptr(),
        ..Table::EMPTY
    };
    unsafe {
        let commands = table(actor);
        let active = core::ptr::from_ref(&commands_for_rule);
        let previous = commands.replace(active);
        let clock = (actor + 9440) as *mut f32;
        let origin = clock.replace(0.);
        let buffers = buffer(actor);
        let saved = buffers.replace([0; CAPACITY]);
        let run: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(crate::memory::entry(0x2e9d54));
        let handled = run(actor) != 0;
        // An accepted transition installs its own native command state.
        if commands.read() == active {
            commands.write(previous);
            clock.write(origin);
            buffers.write(saved);
        }
        handled
    }
}
