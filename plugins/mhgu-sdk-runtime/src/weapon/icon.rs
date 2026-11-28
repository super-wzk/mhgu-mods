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

// Both panels draw from atlas 4. A board row is indexed by its position in
// the posted quest, which need not equal the player's room slot.
#[derive(Clone, Copy)]
enum RoomPanel {
    Player(u32),
    Board { panel: u32, quest: u32 },
}

static ROOM_PANEL: Shared<Option<RoomPanel>> = Shared::new(None);

fn during_room_draw(source: RoomPanel, render: impl FnOnce() -> u32) -> u32 {
    let previous = ROOM_PANEL.with(|p| p.replace(source));
    let result = render();
    ROOM_PANEL.with(|p| *p = previous);
    result
}

#[forge::hook(offset = 0x60db68)]
unsafe fn player_info(panel: u32) -> u32 {
    during_room_draw(RoomPanel::Player(panel), || original!(panel))
}

#[forge::hook(offset = 0x61225c)]
unsafe fn quest_board(panel: u32, mode: u32, index: u32, quests: u32, page: u32) -> u32 {
    let quest = quests.wrapping_add(index.wrapping_mul(10328));
    during_room_draw(RoomPanel::Board { panel, quest }, || original!(panel, mode, index, quests, page))
}

unsafe fn room_icon(panel: u32, group: u32, part: u32) -> Option<u8> {
    let base = forge::mem::text_addr() as u32;
    unsafe {
        let slot = match ROOM_PANEL.with(|p| *p)? {
            RoomPanel::Player(owner) if owner == panel && part == 11 && group == read(panel + 80) =>
                read(panel + 136),
            RoomPanel::Board { panel: owner, quest } if owner == panel && part == 2 => {
                let row = (0..4).find(|i| read(base + 0x163476c + 4 * i) == group)?;
                read(quest + 10280 + 12 * row)
            }
            _ => return None,
        };
        let manager = read(base + 0x1885b58);
        let session = read(base + 0x188ca48);
        if slot >= 4 || manager == 0 || session == 0 {
            return None;
        }
        let equipment = if slot == gu!(0x3f8398, session) {
            gu!(0x275a30, manager)
        } else {
            gu!(0x27617c, manager, slot)
        };
        crate::weapon::records::display(equipment).map(|w| w.definition.kind.icon)
    }
}

#[forge::hook(offset = 0x56e6a8)]
unsafe fn draw(panel: u32, group: u32, part: u32, atlas: u32, icon: u32) -> u32 {
    let slot = match atlas {
        0 | 1 => u8::try_from(icon).ok(),
        4 => unsafe { room_icon(panel, group, part) },
        _ => None,
    };
    let Some(slot) = slot else {
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
    if atlas == 4 { original!(panel, group, part, atlas, icon) } else { original!(panel, group, part, atlas, BLANK) }
}

// The quest member panel draws hunters from atlas 4 and companions from atlas 5.
// Keep GU's player tint and only replace the hunter sprite on the drawn row.
#[forge::hook(offset = 0x634124)]
unsafe fn member_icon(panel: u32) -> u32 {
    let result = original!(panel);
    unsafe {
        let base = forge::mem::text_addr() as u32;
        let slot = read(panel + 76);
        if read(panel + 80) == 0 && slot < 4 {
            let manager = read(base + 0x1885b58);
            if manager != 0 {
                let actor = gu!(0x274ce4, manager, slot);
                if actor != 0 && gu!(0x27c6f8, actor) == 0 {
                    let equipment = read(actor + 5060);
                    if let Some(weapon) = crate::weapon::records::display(equipment)
                        .or_else(|| crate::weapon::profile::peer_model(actor, equipment))
                    {
                        let part = read(base + 0x189fe2c + 32 * read(panel + 84));
                        gu!(0x56e6a8, panel, read(panel + 88), part, 0, u32::from(weapon.definition.kind.icon));
                    }
                }
            }
        }
    }
    result
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, player_info);
    forge::install_hook!(base, quest_board);
    forge::install_hook!(base, member_icon);
    forge::install_hook!(base, draw);
    forge::install_hook!(base, stock_crop);
}
