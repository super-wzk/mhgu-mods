//! Attack button state produced by GU's 2A79CC and 27CA28 input pipeline.
use crate::memory::entry;

const ATTACK_BUTTONS: u16 = 0x120; // X + A
const FILTERED_HELD: u32 = 5108;
const FILTERED_PRESSED: u32 = 5112;
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
