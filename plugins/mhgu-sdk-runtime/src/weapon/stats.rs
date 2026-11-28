//! GU 1.4 weapon stat readers used by equipment details and DEE34's
//! transient hunter stat calculation. Never edit the 44-byte save record.
use super::inventory;
use crate::weapon::records;

// DDD48 also runs outside DEE34. GU writes the resulting points through
// record+40; only the three jewel IDs in its input need substitution.
#[forge::hook(offset = 0xddd48)]
unsafe fn skill_points(record: u32) -> u32 {
    if let Some(w) = unsafe { records::for_record(record) } {
        let mut copy = unsafe { (record as *const [u32; 11]).read_unaligned() };
        let ptr = copy.as_mut_ptr() as u32;
        unsafe {
            ((ptr + 10) as *mut [u16; 3]).write_unaligned(w.jewels);
        }
        return records::with_record(ptr, w, || original!(ptr));
    }
    original!(record)
}

#[forge::hook(offset = 0xdb028)]
unsafe fn upgrade_level(record: u32) -> u32 {
    unsafe { records::for_record(record) }
        .map_or_else(|| original!(record), |w| u32::from(w.upgrade) + 1)
}

#[forge::hook(offset = 0xdb7e0)]
unsafe fn attack(r: u32) -> i32 {
    unsafe { records::for_record(r) }.map_or_else(|| original!(r), |s| i32::from(s.level().attack))
}
#[forge::hook(offset = 0xdb88c)]
unsafe fn affinity(r: u32) -> i32 {
    unsafe { records::for_record(r) }
        .map_or_else(|| original!(r), |s| i32::from(s.level().affinity))
}
#[forge::hook(offset = 0xdb900)]
unsafe fn sharpness_level(r: u32) -> u32 {
    unsafe { records::for_record(r) }
        .map_or_else(|| original!(r), |s| u32::from(s.level().sharpness_level))
}
#[forge::hook(offset = 0xdbe74)]
unsafe fn defense(r: u32) -> u32 {
    unsafe { records::for_record(r) }.map_or_else(|| original!(r), |s| u32::from(s.level().defense))
}
#[forge::hook(offset = 0xdb984)]
unsafe fn element(r: u32, hand: usize) -> u32 {
    unsafe { records::for_record(r) }.map_or_else(
        || original!(r, hand),
        |s| {
            s.definition
                .equipment
                .elements
                .get(hand)
                .map_or(0, |e| e.id().into())
        },
    )
}
#[forge::hook(offset = 0xdba5c)]
unsafe fn magnitude(r: u32, hand: usize) -> i32 {
    unsafe { records::for_record(r) }.map_or_else(
        || original!(r, hand),
        |s| {
            s.level()
                .element_values
                .get(hand)
                .copied()
                .unwrap_or(0)
                .into()
        },
    )
}

#[forge::hook(offset = 0xde16c)]
unsafe fn special(record: u32) -> u32 {
    if let Some(s) = unsafe { records::for_record(record) } {
        return s.definition.equipment.special_skill.into();
    }
    original!(record)
}
#[forge::hook(offset = 0xde1c0)]
unsafe fn frenzy_affinity(record: u32) -> bool {
    if let Some(s) = unsafe { records::for_record(record) } {
        return s.definition.equipment.special_skill == 3;
    }
    original!(record)
}
/// F4724 reads the seven cumulative boundaries of the selected kireajiData row.
pub(super) fn sharpness_curve(kind: u8) -> Option<[u16; 7]> {
    unsafe {
        let base = forge::mem::text_addr();
        let tables = *((base + 0x1856b18) as *const u32);
        if tables == 0 || *((tables + 252) as *const u32) == 0 {
            return None;
        }
        let lookup: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(base + 0x13f290);
        let row = lookup(tables, kind.into());
        if row == 0 {
            None
        } else {
            Some(((row + 4) as *const [u16; 7]).read_unaligned())
        }
    }
}
#[forge::hook(offset = 0x29cdfc)]
unsafe fn sharpness_grade(actor: u32, remaining: u32) -> u32 {
    if actor != 0
        && let Some(shown) = unsafe { records::display(inventory::actor_equipment(actor)) }
        && let Some(curve) =
            sharpness_curve(shown.definition.levels[usize::from(shown.upgrade)].sharpness_type)
    {
        return curve[..6].partition_point(|&limit| u32::from(limit) < remaining) as u32;
    }
    original!(actor, remaining)
}
pub(super) fn install(base: u32) {
    forge::install_hook!(base, upgrade_level);
    forge::install_hook!(base, skill_points);
    forge::install_hook!(base, special);
    forge::install_hook!(base, frenzy_affinity);
    forge::install_hook!(base, sharpness_grade);
    forge::install_hook!(base, attack);
    forge::install_hook!(base, affinity);
    forge::install_hook!(base, sharpness_level);
    forge::install_hook!(base, defense);
    forge::install_hook!(base, element);
    forge::install_hook!(base, magnitude);
}
