//! Route GU's target camera, map and chat shortcuts.
use mhgu_sdk::weapon::style::input::auxiliary_shoulder_mask;

const UI_PAD: u32 = 0x188c670;
const UI_PRESSED_BUTTONS: usize = 27556;
const PLAYER_SETTINGS: u32 = 0x1897f78;
const CONTROL_TYPE: usize = 17516;
const SWAP_SHOULDERS: usize = 17518;
const CAMERA_PAD: usize = 17519;
const STICK_CAMERA: usize = 17403;
// sPad's game button mapping table (162AA24).
const L3: u32 = 0x0400_0000;
const R3: u32 = 0x0800_0000;

unsafe fn with_pressed_buttons(
    replace: u32,
    remap: impl FnOnce(u32) -> u32,
    read_ui: impl FnOnce() -> u32,
) -> u32 {
    unsafe {
        let base = forge::mem::text_addr();
        let pad = ((base + UI_PAD) as *const *mut u8).read();
        if pad.is_null() {
            return read_ui();
        }
        let pressed = pad.add(UI_PRESSED_BUTTONS).cast::<u32>();
        let buttons = pressed.read();
        let mapped = (buttons & !replace) | remap(buttons);
        if mapped == buttons {
            return read_ui();
        }
        pressed.write(mapped);
        let result = read_ui();
        pressed.write((pressed.read() & !replace) | (buttons & replace));
        result
    }
}

unsafe fn settings() -> *const u8 {
    unsafe { ((forge::mem::text_addr() + PLAYER_SETTINGS) as *const *const u8).read() }
}

// The quest camera performs both target selection and target cycling here.
#[forge::hook(offset = 0x1fec4)]
unsafe fn target_camera(camera: u32) -> u32 {
    unsafe {
        let settings = crate::shortcuts::settings();
        if settings.is_null() {
            return original!(camera);
        }
        let target = match settings.add(CONTROL_TYPE).read() {
            0 if settings.add(SWAP_SHOULDERS).read() == 0 => 0x200,
            0 => 0x400,
            1 if settings.add(CAMERA_PAD).read() == 0 => 2,
            1 => 0x20_0000,
            3 if settings.add(STICK_CAMERA).read() == 0 => L3,
            _ => return original!(camera),
        };
        with_pressed_buttons(
            target,
            |buttons| if buttons & R3 != 0 { target } else { 0 },
            || original!(camera),
        )
    }
}

// Control type 3 uses the shared menu update's virtual chat opener.
#[forge::hook(offset = 0x66f0a8)]
unsafe fn menu_update(menu: u32, command: u32) -> u32 {
    unsafe {
        let settings = crate::shortcuts::settings();
        if !settings.is_null() && settings.add(CONTROL_TYPE).read() == 3 {
            return with_pressed_buttons(
                auxiliary_shoulder_mask(),
                |_| 0,
                || original!(menu, command),
            );
        }
    }
    original!(menu, command)
}

#[forge::hook(offset = 0x678894)]
unsafe fn chat_update(menu: u32) -> u32 {
    unsafe { with_pressed_buttons(auxiliary_shoulder_mask(), |_| 0, || original!(menu)) }
}

// GU checks the shortcut both while the menu is closed and when it is active.
#[forge::hook(offset = 0x793424)]
unsafe fn touch_menu_update(menu: u32) -> u32 {
    unsafe { with_pressed_buttons(auxiliary_shoulder_mask(), |_| 0, || original!(menu)) }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, target_camera);
    forge::install_hook!(base, menu_update);
    forge::install_hook!(base, chat_update);
    forge::install_hook!(base, touch_menu_update);
}
