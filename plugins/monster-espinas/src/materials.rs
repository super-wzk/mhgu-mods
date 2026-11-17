//! GU inventory stores item IDs, so use unused retail rows rather than exceeding its capacity.
use super::u32at;
use core::ffi::CStr;

struct Material {
    id: u16,
    icon: u8,
    color: u8,
    rarity: u8,
    stack: u8,
    sell: u32,
    buy: u32,
    names: [&'static CStr; 7],
}
include!(concat!(env!("OUT_DIR"), "/materials.rs"));

#[forge::hook(offset = 0x192340)]
unsafe fn load(table: u32, stream: u32) -> u32 {
    let result = original!(table, stream);
    if result == 0 {
        return result;
    }
    unsafe {
        let count = u32at(table, 100);
        let rows = u32at(table, 108) as *mut u8;
        if rows.is_null() {
            return result;
        }
        for m in MATERIALS {
            if u32::from(m.id) >= count {
                continue;
            }
            let target = rows.add(56 * usize::from(m.id));
            // Keep the item's identity and sort slot. No inventory row of
            // another species supplies its properties or crafting memberships.
            if target.add(8).cast::<u32>().read_unaligned() != u32::from(m.id) {
                continue;
            }
            target.add(13).write(m.rarity);
            target.add(14).write(m.stack);
            target.add(24).write(m.icon);
            target.add(25).write(m.color);
            target.add(28).cast::<u32>().write_unaligned(m.sell);
            target.add(32).cast::<u32>().write_unaligned(m.buy);
        }
    }
    result
}

// The item-name table has two messages per item. Other GMDs also legitimately
// use these indices: match the decoded GMD's internal name before dispatch.
pub(super) unsafe fn text(gmd: u32, index: u32) -> Option<&'static CStr> {
    let id = u16::try_from(index / 2).ok()?;
    let m = MATERIALS.iter().find(|m| m.id == id)?;
    unsafe {
        let name = u32at(gmd, 140) as *const core::ffi::c_char;
        if name.is_null() || CStr::from_ptr(name) != c"itemData" {
            return None;
        }
        let lang = match u32at(gmd, 104) {
            8 => 1,
            7 => 2,
            2 => 3,
            4 => 4,
            5 => 5,
            3 => 6,
            _ => 0,
        };
        if index & 1 == 0 {
            Some(m.names[lang])
        } else {
            Some(
                [
                    c"Material obtained from an Espinas.",
                    c"从棘龙身上取得的素材。",
                    c"從棘龍身上取得的素材。",
                    c"Matériau issu d’un Espinas.",
                    c"Material von einem Espinas.",
                    c"Materiale ottenuto da un Espinas.",
                    c"Material obtenido de un Espinas.",
                ][lang],
            )
        }
    }
}
pub(super) fn install(base: u32) {
    forge::install_hook!(base, load);
}
