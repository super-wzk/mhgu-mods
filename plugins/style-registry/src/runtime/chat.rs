//! Suppress the auxiliary-shoulder chat shortcut in GU's UI input handlers.
use mhgu_sdk::style::input::auxiliary_shoulder_mask;

// GU 1.4's UI pad singleton and its pressed-button word. The hunter has a
// separate command buffer, so the style still sees the auxiliary press.
const UI_PAD: u32 = 0x188c670;
const UI_PRESSED_BUTTONS: usize = 27556;
const PLAYER_SETTINGS: u32 = 0x1897f78;
const CONTROL_TYPE: usize = 17516;

unsafe fn without_shortcut(read_ui: impl FnOnce() -> u32) -> u32 {
    unsafe {
        let base = forge::mem::text_addr();
        let pad = ((base + UI_PAD) as *const *mut u8).read();
        if pad.is_null() {
            return read_ui();
        }
        let pressed = pad.add(UI_PRESSED_BUTTONS).cast::<u32>();
        let buttons = pressed.read();
        let shortcut = buttons & auxiliary_shoulder_mask();
        if shortcut == 0 {
            return read_ui();
        }
        pressed.write(buttons & !shortcut);
        let result = read_ui();
        pressed.write(pressed.read() | shortcut);
        result
    }
}

// Control type 3 uses the shared menu update's virtual chat opener.
#[forge::hook(offset = 0x66f0a8)]
unsafe fn menu_update(menu: u32, command: u32) -> u32 {
    let settings =
        unsafe { ((forge::mem::text_addr() + PLAYER_SETTINGS) as *const *const u8).read() };
    if !settings.is_null() && unsafe { settings.add(CONTROL_TYPE).read() } == 3 {
        return unsafe { crate::runtime::chat::without_shortcut(|| original!(menu, command)) };
    }
    original!(menu, command)
}

#[forge::hook(offset = 0x678894)]
unsafe fn chat_update(menu: u32) -> u32 {
    unsafe { crate::runtime::chat::without_shortcut(|| original!(menu)) }
}

// GU checks the shortcut both while the menu is closed and when it is active.
#[forge::hook(offset = 0x793424)]
unsafe fn touch_menu_update(menu: u32) -> u32 {
    unsafe { crate::runtime::chat::without_shortcut(|| original!(menu)) }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, menu_update);
    forge::install_hook!(base, chat_update);
    forge::install_hook!(base, touch_menu_update);
}
