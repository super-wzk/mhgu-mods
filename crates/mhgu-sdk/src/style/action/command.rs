//! Input branches bound to a GU command table for the current motion.

/// Largest literal frame before GU's end-of-motion sentinel.
pub const MAX_COMMAND_FRAME: i16 = 998;

/// A resolved input from one of GU's command banks.
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum CommandInput {
    Common(u8),
    Weapon(u8),
    /// A weapon command that waits for GU to resolve X+A.
    WeaponAfterCombo(u8),
    /// The weapon's X+A command, also accepting held X+A or shoulder + X+A.
    AttackChord(u8),
}

/// A branch into a shared GU action or a registered style action.
/// Frames use GU's motion clock.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct BufferedCommand {
    pub group: u16,
    pub slot: u16,
    pub input: CommandInput,
    pub accept_from: i16,
    pub execute_from: i16,
    /// First motion frame that refuses new input. Already buffered input may execute.
    pub accept_until: i16,
}

/// Ordered alternatives. The first accepted input is kept in GU's buffer
/// until execution or the next command-table binding.
#[repr(C)]
pub struct CommandSet {
    rows: *const BufferedCommand,
    count: usize,
    replaced_weapon_slot: Option<u16>,
    retained_native: Option<(u16, &'static [u16])>,
}

// Constructed only from immutable process-lifetime slices.
unsafe impl Sync for CommandSet {}

impl CommandSet {
    pub const fn new(rows: &'static [BufferedCommand]) -> Self {
        Self {
            rows: rows.as_ptr(),
            count: rows.len(),
            replaced_weapon_slot: None,
            retained_native: None,
        }
    }

    /// Remove the specified native weapon destination when these rows are bound.
    pub const fn replacing_weapon(mut self, slot: u16) -> Self {
        self.replaced_weapon_slot = Some(slot);
        self
    }

    /// Keep only the given native destinations from the bound table.
    pub const fn retaining_native(mut self, group: u16, slots: &'static [u16]) -> Self {
        self.retained_native = Some((group, slots));
        self
    }

    #[doc(hidden)]
    pub fn keeps_native(&self, destination: Option<(u16, u16)>) -> bool {
        if let Some((group, slots)) = self.retained_native {
            return destination.is_some_and(|(g, slot)| g == group && slots.contains(&slot));
        }
        !self
            .replaced_weapon_slot
            .is_some_and(|slot| destination == Some((super::GROUP, slot)))
    }

    #[doc(hidden)]
    pub fn rows(&self) -> &'static [BufferedCommand] {
        unsafe { core::slice::from_raw_parts(self.rows, self.count) }
    }
}

/// Return a static set to edit the table GU just selected for the hunter.
/// A null pointer keeps GU's table. The callback runs on the game thread.
pub type Bind = unsafe extern "C" fn(actor: u32, table_id: u32) -> *const CommandSet;

#[cfg(all(target_arch = "arm", target_endian = "little"))]
#[doc(hidden)]
pub mod native {
    use super::{CommandInput, MAX_COMMAND_FRAME};
    use core::ptr;

    pub const CAPACITY: usize = 48;
    pub type Buffer = [u8; CAPACITY];
    pub const WEAPON_CONDITION: u16 = 8;
    pub const WEAPON_AFTER_COMBO: u16 = 0x100 | WEAPON_CONDITION;
    pub const ATTACK_CHORD: u16 = 0x200 | WEAPON_CONDITION;

    impl CommandInput {
        pub const fn opcode_and_id(self) -> (u16, u8) {
            match self {
                Self::Common(id) => (6, id),
                Self::Weapon(id) => (WEAPON_CONDITION, id),
                Self::WeaponAfterCombo(id) => (WEAPON_AFTER_COMBO, id),
                Self::AttackChord(id) => (ATTACK_CHORD, id),
            }
        }
    }

    // cPlCmdTbl -> cPlCmd -> rPlBaseCmd, as read by 26D42C / 2731B4.
    #[derive(Clone, Copy)]
    #[repr(C)]
    pub struct Table {
        pub reserved: [u32; 2],
        pub count: u32,
        pub capacity: u32,
        pub flags: u32,
        pub entries: *const *const Entry,
    }

    #[repr(C)]
    pub struct Entry {
        pub vtable: u32,
        pub enabled: u32,
        pub resource: *const Resource,
        pub pre_time: f32,
        pub start: f32,
        pub end: f32,
    }

    impl Entry {
        pub const EMPTY: Self = Self::new(ptr::null(), 0, 0, MAX_COMMAND_FRAME);

        pub const fn new(resource: *const Resource, pre_time: i16, start: i16, end: i16) -> Self {
            Self {
                vtable: 0,
                enabled: 1,
                resource,
                pre_time: pre_time as f32,
                start: start as f32,
                end: end as f32,
            }
        }
    }

    #[repr(C)]
    pub struct Resource {
        pub header: [u32; 25],
        pub command: Command,
    }

    impl Resource {
        pub const EMPTY: Self = Self {
            header: [0; 25],
            command: Command {
                group: 0,
                slot: 0,
                flags: 0,
                condition: 0,
                parameters: [0; 4],
            },
        };
    }

    #[derive(Clone, Copy)]
    #[repr(C)]
    pub struct Command {
        pub group: u16,
        pub slot: u16,
        pub flags: u16,
        pub condition: u16,
        pub parameters: [u32; 4],
    }

    impl Table {
        pub const EMPTY: Self = Self {
            reserved: [0; 2],
            count: 0,
            capacity: 0,
            flags: 0,
            entries: ptr::null(),
        };
    }

    pub const fn table(actor: u32) -> *mut *const Table {
        (actor + 9384) as *mut *const Table
    }

    pub const fn buffer(actor: u32) -> *mut Buffer {
        (actor + 9392) as *mut Buffer
    }
}
