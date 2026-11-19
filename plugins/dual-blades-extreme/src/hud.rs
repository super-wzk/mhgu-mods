//! Frontier's release gauge and four blade-polish marks on GU's twin-sword HUD.
use crate::{battle, local_actor, native, program, read, write};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};

const PRIVATE: u32 = 0x4000_0002;
const STOCK_TEXTURE: u32 = 1;
const STOCK_RECT: [f32; 4] = [384., 112., 512., 136.];
static ATLAS: AtomicU32 = AtomicU32::new(0);

unsafe fn atlas() -> Option<u32> {
    let cached = ATLAS.load(Ordering::Acquire);
    if cached != 0 {
        return Some(cached);
    }
    unsafe {
        let base = forge::mem::text_addr();
        let manager = read::<u32>(base, 0x2126a64);
        if manager == 0 {
            return None;
        }
        let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
            core::mem::transmute(read::<u32>(read(manager, 0), 48));
        let texture = load(
            manager,
            base + 0x21255d0,
            c"GUI\\frontier_db\\gauge".as_ptr(),
            1,
        );
        if texture == 0 {
            return None;
        }
        if read::<u32>(texture, 80) & 1 == 0 || read::<[u32; 2]>(texture, 220) != [128, 1024] {
            native!(0x887660, texture);
            return None;
        }
        // B80FD0 GUI descriptor. Retain one native reference while the HUD can draw it.
        let mut desc = Box::new([0u32; 16]);
        desc[0] = PRIVATE;
        desc[1] = 4;
        desc[3] = (1024 << 16) | 128;
        desc[10] = texture;
        let ptr = Box::into_raw(desc);
        match ATLAS.compare_exchange(0, ptr as u32, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Some(ptr as u32),
            Err(existing) => {
                drop(Box::from_raw(ptr));
                native!(0x887660, texture);
                Some(existing)
            }
        }
    }
}

unsafe fn plate(layout: u32, frame: Option<u32>) {
    unsafe {
        if layout == 0 || (frame.is_none() && ATLAS.load(Ordering::Relaxed) == 0) {
            return;
        }
        // hud4_t_sword group 0: child 1 is the 128 x 24 backing sprite,
        // child 2 is the native gauge polygon. GU owns both widgets.
        let groups = read::<u32>(
            layout,
            if read::<u32>(layout, 664) != 0 {
                248
            } else {
                660
            },
        );
        if groups == 0 {
            return;
        }
        let group = read::<u32>(groups, 0);
        if group == 0 {
            return;
        }
        let children = read::<u32>(group, 216);
        if children == 0 {
            return;
        }
        let sprite = read::<u32>(children, 4);
        if sprite == 0 {
            return;
        }
        let id = read::<u32>(sprite, 256);
        let rect = if let Some(frame) = frame {
            if id != PRIVATE {
                if id != STOCK_TEXTURE || read::<[f32; 4]>(sprite, 268) != STOCK_RECT {
                    return;
                }
                let Some(desc) = atlas() else { return };
                write(sprite, 256, PRIVATE);
                write(sprite, 260, desc);
            }
            let top = (32 * frame) as f32;
            [0., top, 128., top + 24.]
        } else if id == PRIVATE {
            native!(0xa4faf0, sprite, STOCK_TEXTURE);
            STOCK_RECT
        } else {
            return;
        };
        native!(0xa4fb58, sprite, rect.as_ptr() as u32);
    }
}

#[forge::hook(offset = 0x64b830)]
unsafe fn twin_sword_gauge(widget: u32) -> u32 {
    unsafe {
        let layout = read::<u32>(widget, 36);
        let Some(actor) = local_actor().filter(|&actor| read::<i16>(actor, 5078) > 0) else {
            plate(layout, None);
            return original!(widget);
        };
        let max = read::<f32>(widget, 124);
        if layout == 0 || !max.is_finite() || max <= 0. {
            return original!(widget);
        }
        let (phase, points, polish) = battle::hud_state(actor);
        let full = points == program::FLAME_MAX;
        let state = u32::from(phase) + if full && phase >= 2 { 2 } else { 0 };
        native!(0x6578f8, widget);
        write(widget, 116, max * points as f32 / program::FLAME_MAX as f32);
        write(widget, 132, if full { 5u32 } else { 1u32 });
        write(widget, 148, u8::from(full));
        native!(0x54e344, widget + 96);
        let result = native!(0x54e38c, widget + 96, layout);
        plate(layout, Some(5 * state + polish));
        result
    }
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), twin_sword_gauge);
}
