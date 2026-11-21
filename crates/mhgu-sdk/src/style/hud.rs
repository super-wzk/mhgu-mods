//! Native GU weapon-gauge backing sprites.
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::memory::entry;
use core::ffi::CStr;

unsafe fn read<T: Copy>(p: u32, off: u32) -> T {
    unsafe { ((p + off) as *const T).read_unaligned() }
}
unsafe fn write<T>(p: u32, off: u32, value: T) {
    unsafe {
        ((p + off) as *mut T).write_unaligned(value);
    }
}
unsafe fn release(p: u32) {
    unsafe {
        let f: unsafe extern "C" fn(u32) = core::mem::transmute(entry(0x887660));
        f(p);
    }
}

/// A private 128 x 1024 HUD texture, drawn through the existing GU weapon widget.
pub struct GaugePlate {
    id: u32,
    path: &'static CStr,
    stock: [f32; 4],
    columns: u32,
    cell: [u32; 2],
    atlas: AtomicU32,
    stock_id: AtomicU32,
}
impl GaugePlate {
    pub const fn new(id: u32, path: &'static CStr, stock: [f32; 4]) -> Self {
        Self {
            id,
            path,
            stock,
            columns: 1,
            cell: [128, 24],
            atlas: AtomicU32::new(0),
            stock_id: AtomicU32::new(0),
        }
    }

    /// Select the visible crop in each 32-pixel-high row of a private sheet.
    pub const fn with_cells(mut self, columns: u32, cell: [u32; 2]) -> Self {
        assert!(
            columns > 0 && cell[0] > 0 && columns * cell[0] <= 8191 && cell[1] > 0 && cell[1] <= 32
        );
        self.cell = cell;
        self.columns = columns;
        self
    }

    unsafe fn atlas(&self) -> Option<u32> {
        let cached = self.atlas.load(Ordering::Acquire);
        if cached != 0 {
            return Some(cached);
        }
        unsafe {
            let base = entry(0) as u32;
            let manager = read::<u32>(base, 0x2126a64);
            if manager == 0 {
                return None;
            }
            let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(read::<u32>(read(manager, 0), 48));
            let texture = load(manager, base + 0x21255d0, self.path.as_ptr(), 1);
            if texture == 0 {
                return None;
            }
            if read::<u32>(texture, 80) & 1 == 0
                || read::<[u32; 2]>(texture, 220) != [self.cell[0] * self.columns, 1024]
            {
                release(texture);
                return None;
            }
            // B80FD0 GUI descriptor. Retain one native reference while the HUD can draw it.
            let mut desc = Box::new([0u32; 16]);
            desc[0] = self.id;
            desc[1] = 4;
            desc[3] = (1024 << 16) | (self.cell[0] * self.columns);
            desc[10] = texture;
            let ptr = Box::into_raw(desc);
            match self
                .atlas
                .compare_exchange(0, ptr as u32, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => Some(ptr as u32),
                Err(existing) => {
                    drop(Box::from_raw(ptr));
                    release(texture);
                    Some(existing)
                }
            }
        }
    }

    /// Update the weapon HUD backing. A missing frame restores its original texture.
    /// # Safety
    /// Called on the GUI thread with the live GU hud4 or hud5 layout.
    pub unsafe fn show(&self, layout: u32, frame: Option<u32>) {
        unsafe {
            if layout == 0 || (frame.is_none() && self.atlas.load(Ordering::Relaxed) == 0) {
                return;
            }
            let Some(children) = weapon_children(layout) else {
                return;
            };
            let sprite = read::<u32>(children, 4);
            if sprite == 0 {
                return;
            }
            self.show_sprite(sprite,frame,self.stock);
        }
    }

    /// Apply a private cell to a known weapon sprite, or restore its captured native crop.
    /// # Safety
    /// The sprite must be live on the GUI thread and owned by this gauge.
    pub unsafe fn show_sprite(&self,sprite:u32,frame:Option<u32>,stock:[f32;4]){
        unsafe {
            let id = read::<u32>(sprite, 256);
            let rect = if let Some(frame) = frame {
                if id != self.id {
                    if read::<[f32; 4]>(sprite, 268) != stock {
                        return;
                    }
                    let Some(desc) = self.atlas() else { return };
                    self.stock_id.store(id, Ordering::Relaxed);
                    write(sprite, 256, self.id);
                    write(sprite, 260, desc);
                }
                let top = (32 * (frame / self.columns)) as f32;
                let left = (self.cell[0] * (frame % self.columns)) as f32;
                [
                    left,
                    top,
                    left + self.cell[0] as f32,
                    top + self.cell[1] as f32,
                ]
            } else if id == self.id {
                let bind: unsafe extern "C" fn(u32, u32) = core::mem::transmute(entry(0xa4faf0));
                bind(sprite, self.stock_id.load(Ordering::Relaxed));
                stock
            } else {
                return;
            };
            let crop: unsafe extern "C" fn(u32, *const f32) = core::mem::transmute(entry(0xa4fb58));
            crop(sprite, rect.as_ptr());
        }
    }
}

/// # Safety
/// The layout must be a live GU weapon HUD from its update callback.
pub unsafe fn weapon_children(layout: u32) -> Option<u32> {
    unsafe {
        if layout == 0 {
            return None;
        }
        let groups = read::<u32>(
            layout,
            if read::<u32>(layout, 664) != 0 {
                248
            } else {
                660
            },
        );
        if groups == 0 {
            return None;
        }
        let group = read::<u32>(groups, 0);
        if group == 0 {
            return None;
        }
        let children = read::<u32>(group, 216);
        (children != 0).then_some(children)
    }
}
