//! Button state produced by GU's 2A79CC and 27CA28 input pipeline.
use crate::memory::entry;

const ATTACK_BUTTONS: u16 = 0x120; // X + A
const FILTERED_HELD: u32 = 5108;
const FILTERED_PRESSED: u32 = 5112;
const NATIVE_HELD: u32 = 5116;
const NATIVE_PRESSED: u32 = 5124;
const COMBO_STATE: u32 = 5188;
const COMBO_PENDING: u32 = 1;
const SHOULDER_CHORD: u32 = 0x400;
const COMBO_BUTTON_SHIFT: u32 = 13;

/// Resolve the auxiliary shoulder button for GU's current controller layout.
///
/// # Safety
/// Call on the game thread with GU's input settings initialized.
pub unsafe fn auxiliary_shoulder_mask() -> u32 {
    let resolve: unsafe extern "C" fn() -> u32 = unsafe { core::mem::transmute(entry(0x65fa04)) };
    unsafe { resolve() }
}

/// Physical Special Attack key selected by GU 27CA28, in addition to the virtual 0x1000 key.
///
/// # Safety
/// Call on the game thread with the input settings initialized.
pub unsafe fn special_attack_mask() -> u16 {
    let settings = unsafe { (entry(0x1897f78) as *const u32).read() };
    if settings != 0 && unsafe { ((settings + 17517) as *const u8).read() } == 1 {
        0x2000
    } else {
        0x4000
    }
}

/// Remove early GU shoulder gestures whose physical key belongs to a drawn style.
/// Return true when the native item selector owns the face buttons.
///
/// # Safety
/// Call from the local style input filter after 27E970 and 27ECCC.
pub unsafe fn reserve_shoulder_gestures(actor: u32, shoulders: u16, physical: u16) -> bool {
    unsafe {
        let resolve: unsafe extern "C" fn() -> u32 = core::mem::transmute(entry(0x65f8ec));
        let item_key = resolve() as u16;
        let flags = (actor + 5172) as *mut u32;
        if shoulders & item_key == 0 && flags.read() & (1 << 6) != 0 {
            return true;
        }
        if physical & shoulders & item_key != 0 {
            // The swapped layout uses ZL for GU's tap/hold item and camera gesture.
            flags.write(flags.read() & !((1 << 0) | (1 << 6) | (1 << 24)));
            ((actor + 5200) as *mut f32).write(0.);
            ((actor + 5208) as *mut u32).write(0);
            ((actor + 5212) as *mut f32).write(0.);
        }
        false
    }
}

/// Whether the resolved auxiliary shoulder is held in GU's filtered state.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn auxiliary_shoulder_held(actor: u32) -> bool {
    let held = unsafe { ((actor + FILTERED_HELD) as *const u16).read() };
    u32::from(held) & unsafe { auxiliary_shoulder_mask() } != 0
}

/// Whether GU is waiting for the rest of a combination involving X or A.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn attack_combo_pending(actor: u32) -> bool {
    let combo = unsafe { ((actor + COMBO_STATE) as *const u32).read() };
    combo & COMBO_PENDING != 0 && (combo >> COMBO_BUTTON_SHIFT) & u32::from(ATTACK_BUTTONS) != 0
}

/// Fresh X and A edges from GU's filtered pad state.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn attack_buttons_pressed(actor: u32) -> u16 {
    unsafe { ((actor + FILTERED_PRESSED) as *const u16).read() & ATTACK_BUTTONS }
}

/// Physical pad buttons after GU's sPad translation table (162AA24).
#[derive(Clone, Copy)]
pub enum PadButton {
    Y = 0x10,
    X = 0x20,
    R = 0x40,
    B = 0x80,
    A = 0x100,
    Zl = 0x200,
    Zr = 0x800,
}

/// The hunter's button snapshot before weapon-specific combination filtering.
pub struct Pad {
    held: u32,
    pressed: u32,
}

impl Pad {
    /// # Safety
    /// The actor must be a live local quest hunter on the game thread.
    pub unsafe fn sample(actor: u32) -> Self {
        unsafe {
            Self {
                held: ((actor + NATIVE_HELD) as *const u32).read(),
                pressed: ((actor + NATIVE_PRESSED) as *const u32).read(),
            }
        }
    }

    pub fn held(&self, button: PadButton) -> bool {
        self.held & button as u32 != 0
    }

    pub fn pressed(&self, button: PadButton) -> bool {
        self.pressed & button as u32 != 0
    }
}

/// Consume the sprint commands produced by 27CA28 while a style uses R as a modifier.
/// The native ground dispatcher still processes walking and ordinary sheathing.
///
/// # Safety
/// The actor must be the live local quest hunter in its controller callback.
pub unsafe fn reserve_dash_shoulder(actor: u32) {
    let clear: unsafe extern "C" fn(u32, u32) = unsafe { core::mem::transmute(entry(0x285e08)) };
    unsafe {
        clear(actor, 0);
        clear(actor, 31);
    }
}

/// Whether GU resolved X+A with the weapon shoulder pressed in the same combination.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn attack_chord_with_shoulder(actor: u32) -> bool {
    unsafe { ((actor + COMBO_STATE) as *const u32).read() & SHOULDER_CHORD != 0 }
}

/// Whether X and A are both held in GU's filtered pad state.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn attack_chord_held(actor: u32) -> bool {
    let held = unsafe { ((actor + FILTERED_HELD) as *const u16).read() };
    held & ATTACK_BUTTONS == ATTACK_BUTTONS
}

/// Whether a fresh attack-button press completed X+A using GU's filtered pad state.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn attack_chord_pressed(actor: u32) -> bool {
    unsafe { attack_chord_held(actor) && attack_buttons_pressed(actor) != 0 }
}

/// Low button banks written by GU 2A79CC after gameplay suppression and before arts and common commands.
#[repr(C)]
pub struct Buttons {
    pub held: u16,
    pub released: u16,
    pub pressed: u16,
    pub previous_pressed: u16,
}

pub type Filter = unsafe extern "C" fn(actor: u32, buttons: &mut Buttons);
const FILTER_NAME: &core::ffi::CStr = c"mhgu::StyleInputFilter";
const FILTER_MAGIC: u32 = u32::from_le_bytes(*b"SPAD");

#[doc(hidden)]
#[repr(C)]
pub struct FilterService(pub unsafe extern "C" fn(u32, Filter) -> bool);

/// # Safety
/// Publish a resident service during registry initialization.
#[doc(hidden)]
pub unsafe fn publish_filter(api: &'static FilterService) -> bool {
    unsafe { crate::service::publish(FILTER_NAME, FILTER_MAGIC, api) }
}

pub fn bind_filter(id: u32, filter: Filter) -> bool {
    crate::service::on_game_thread()
        && unsafe { crate::service::find::<FilterService>(FILTER_NAME, FILTER_MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, filter) })
}
