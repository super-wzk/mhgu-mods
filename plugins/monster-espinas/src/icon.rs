//! A private GUI texture. Native name/atlas tables and resident archives stay intact.
use super::{SPECIES, u32at};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};

const PRIVATE: u32 = 0x4000_0000 | SPECIES.packed() as u32;
static TEXTURE: AtomicU32 = AtomicU32::new(0);

#[forge::hook(offset = 0x560ca4)]
unsafe fn index(table: u32, raw: u32) -> u32 {
    if raw == u32::from(SPECIES.packed()) {
        PRIVATE
    } else {
        original!(table, raw)
    }
}

// Quest cards carry byte codes, not species IDs. The package uses our ID as a
// free non-Hyper code in GU 1.4. The UI subtracts one before calling draw.
#[forge::hook(offset = 0x3a3178)]
unsafe fn quest_index(table: u32, code: u32) -> u32 {
    if code == u32::from(SPECIES.packed()) {
        PRIVATE + 1
    } else {
        original!(table, code)
    }
}

// A GUI texture descriptor is sixteen words (B80FD0). 542AA0 reads the
// rTexture at +40 and the pixel rectangle at +8/+12. Own one descriptor and
// one native resource reference for the life of this NRO, as widgets retain it.
fn texture(base: u32) -> Option<u32> {
    let cached = TEXTURE.load(Ordering::Acquire);
    if cached != 0 {
        return Some(cached);
    }
    unsafe {
        let manager = u32at(base, 0x2126a64);
        if manager == 0 {
            return None;
        }
        let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
            core::mem::transmute(u32at(u32at(manager, 0), 48));
        let tex = load(
            manager,
            base + 0x21255d0,
            c"GUI\\monster\\espinas".as_ptr(),
            1,
        );
        if tex == 0 {
            return None;
        }
        let release: unsafe extern "C" fn(u32) = core::mem::transmute(base + 0x887660);
        if u32at(tex, 80) & 1 == 0 || u32at(tex, 220) != 128 || u32at(tex, 224) != 128 {
            release(tex);
            return None;
        }
        let mut row = Box::new([0u32; 16]);
        row[0] = PRIVATE;
        row[1] = 4; // crop coordinates in pixels, not a material parameter block
        row[3] = (128 << 16) | 128;
        row[10] = tex;
        let addr = Box::into_raw(row) as u32;
        match TEXTURE.compare_exchange(0, addr, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Some(addr),
            Err(winner) => {
                drop(Box::from_raw(addr as *mut [u32; 16]));
                release(tex);
                Some(winner)
            }
        }
    }
}

#[forge::hook(offset = 0x56ef50)]
unsafe fn draw(ui: u32, group: u32, element: u32, icon: u32, atlas: u32) -> u32 {
    if icon != PRIVATE {
        return original!(ui, group, element, icon, atlas);
    }
    // Locate the exact widget as GU 546F48 / 546F84 do. Switching back to
    // any retail icon restores its original texture through A4FAF0.
    unsafe {
        let base = forge::mem::text_addr();
        let root = u32at(ui, 36);
        if root == 0 {
            return 0;
        }
        let count = u32at(root, 664);
        let (list, i) = if group < count {
            (u32at(root, 248), group)
        } else {
            (u32at(root, 660), group - count)
        };
        if list == 0 {
            return 0;
        }
        let part = u32at(list, 4 * i);
        if part == 0 {
            return 0;
        }
        let children = u32at(part, 216);
        if children == 0 {
            return 0;
        }
        let widget = u32at(children, 4 * element);
        if widget == 0 {
            return 0;
        }
        ((widget + 256) as *mut u32).write(PRIVATE);
        let row = texture(base).unwrap_or(0);
        ((widget + 260) as *mut u32).write(row);
        if row == 0 {
            return 0;
        }
        let rect: unsafe extern "C" fn(u32, *const [f32; 4]) -> u32 =
            core::mem::transmute(base + 0xa4fb58);
        rect(widget, &[0., 0., 128., 128.])
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, index);
    forge::install_hook!(base, quest_index);
    forge::install_hook!(base, draw);
}
