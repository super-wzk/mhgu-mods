//! Private icon textures on GU's native category, inventory and trade sprites.
use super::{gu, inventory::RegisteredEquipment, resource, set};
use alloc::{boxed::Box, collections::BTreeMap};
use core::ffi::{CStr, c_char};
use mhgu_sdk::sync::Shared;

const FIRST_PRIVATE: u8 = 128;
const BLANK: u32 = 46;
const PRIVATE_DESCRIPTOR: u32 = u32::MAX;
const SIZE: u32 = 64;

#[derive(Clone, Copy)]
struct Icon {
    path: &'static CStr,
    descriptor: u32,
}

static ICONS: Shared<BTreeMap<u8, Icon>> = Shared::new(BTreeMap::new());

pub(super) unsafe fn equipment(
    panel: u32,
    group: u32,
    part: u32,
    entry: RegisteredEquipment,
) -> u32 {
    unsafe {
        gu!(0x56e6a8, panel, group, part, 0, u32::from(entry.kind.icon));
        gu!(0x56e6fc, panel, group, part, equipment_color(entry))
    }
}

pub(super) unsafe fn equipment_color(entry: RegisteredEquipment) -> u32 {
    unsafe { gu!(0x560ef0, 0, u32::from(entry.equipment.rarity) - 1, 1) }
}

pub(super) unsafe extern "C" fn register(slot: u8, path: *const c_char) -> u32 {
    if slot < FIRST_PRIVATE || path.is_null() {
        return 1;
    }
    let path = unsafe { CStr::from_ptr(path) };
    if !(1..64).contains(&path.to_bytes().len()) {
        return 1;
    }
    ICONS.with(|icons| match icons.entry(slot) {
        alloc::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(Icon {
                path,
                descriptor: 0,
            });
            0
        }
        alloc::collections::btree_map::Entry::Occupied(slot) if slot.get().path == path => 0,
        _ => 2,
    })
}

unsafe fn read(addr: u32) -> u32 {
    unsafe { (addr as *const u32).read_unaligned() }
}

fn descriptor(slot: u8, icon: Icon) -> Option<u32> {
    if icon.descriptor != 0 {
        return Some(icon.descriptor);
    }
    let texture = resource::load(0x21255d0, icon.path)?;
    if unsafe { (read(texture + 220), read(texture + 224)) } != (SIZE, SIZE) {
        return None;
    }
    // The GUI sprite consumes the same native descriptor as weapon HUD sprites.
    Some(ICONS.with(|icons| {
        let icon = icons.get_mut(&slot).unwrap();
        if icon.descriptor == 0 {
            let mut desc = Box::new([0u32; 16]);
            desc[0] = PRIVATE_DESCRIPTOR;
            desc[1] = 4;
            desc[3] = SIZE << 16 | SIZE;
            desc[10] = texture;
            icon.descriptor = Box::into_raw(desc) as u32;
        }
        icon.descriptor
    }))
}

unsafe fn sprite(layout: u32, mut group: u32, part: u32) -> Option<u32> {
    unsafe {
        if layout == 0 {
            return None;
        }
        let stock = read(layout + 664);
        let groups = if group < stock {
            read(layout + 248)
        } else {
            group -= stock;
            read(layout + 660)
        };
        if groups == 0 {
            return None;
        }
        let group = read(groups + group * 4);
        if group == 0 {
            return None;
        }
        let nodes = read(group + 216);
        if nodes == 0 {
            return None;
        }
        let sprite = read(nodes + part * 4);
        (sprite != 0).then_some(sprite)
    }
}

// 56EB24 uses 546F84 directly when a row switches back to retail equipment.
#[forge::hook(offset = 0x546f84)]
unsafe fn stock_crop(layout: u32, group: u32, part: u32, crop: u32) -> u32 {
    unsafe {
        if let Some(sprite) = sprite(layout, group, part) {
            let bound = read(sprite + 260);
            if bound != 0 && read(bound) == PRIVATE_DESCRIPTOR {
                let resources = read(sprite + 120);
                if resources != 0 {
                    set(sprite, 260, gu!(0xb80fd0, resources, read(sprite + 256)));
                }
            }
        }
    }
    original!(layout, group, part, crop)
}

#[forge::hook(offset = 0x56e6a8)]
unsafe fn draw(panel: u32, group: u32, part: u32, atlas: u32, icon: u32) -> u32 {
    if atlas > 1 {
        return original!(panel, group, part, atlas, icon);
    }
    let Some(slot) = u8::try_from(icon).ok() else {
        return original!(panel, group, part, atlas, icon);
    };
    let private = ICONS.with(|icons| icons.get(&slot).copied());
    let Some(private) = private else {
        return original!(panel, group, part, atlas, icon);
    };
    unsafe {
        if let Some(sprite) = sprite(read(panel + 36), group, part)
            && let Some(custom) = descriptor(slot, private)
        {
            set(sprite, 260, custom);
            return gu!(
                0xa4fb58,
                sprite,
                [0., 0., SIZE as f32, SIZE as f32].as_ptr() as u32
            );
        }
    }
    original!(panel, group, part, atlas, BLANK)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, draw);
    forge::install_hook!(base, stock_crop);
}
