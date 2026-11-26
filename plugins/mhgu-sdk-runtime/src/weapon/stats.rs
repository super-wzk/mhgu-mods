//! GU 1.4 weapon stat readers used by equipment details and DEE34's
//! transient hunter stat calculation. Never edit the 44-byte save record.
use super::{
    inventory::{self, RegisteredEquipment},
    profile,
};
use mhgu_sdk::{sync::Shared, weapon::EquipmentLevel};

#[derive(Clone, Copy)]
struct Source {
    record: u32,
    equipment: RegisteredEquipment,
    level: EquipmentLevel,
}
static SOURCE: Shared<Option<Source>> = Shared::new(None);

pub(super) fn with_record<T>(
    record: u32,
    equipment: RegisteredEquipment,
    level: EquipmentLevel,
    f: impl FnOnce() -> T,
) -> T {
    let prev = SOURCE.with(|s| {
        s.replace(Source {
            record,
            equipment,
            level,
        })
    });
    let value = f();
    SOURCE.with(|s| *s = prev);
    value
}

fn source(record: u32) -> Option<Source> {
    SOURCE.with(|s| s.filter(|s| s.record == record))
}

#[forge::hook(offset = 0xdee34)]
unsafe fn calculate(stats: u32) -> u32 {
    // DEE34 receives the equipment aggregate at hunter equipment +4.
    if !crate::save::character_loaded() {
        return original!(stats);
    }
    if let Some(equipment) = stats.checked_sub(4)
        && let Some(weapon) = unsafe { profile::display(equipment) }
    {
        let e = weapon.definition;
        return with_record(stats + 20, e, e.levels[usize::from(weapon.upgrade)], || {
            original!(stats)
        });
    }
    original!(stats)
}

#[forge::hook(offset = 0xdb7e0)]
unsafe fn attack(r: u32) -> i32 {
    source(r).map_or_else(|| original!(r), |s| i32::from(s.level.attack))
}
#[forge::hook(offset = 0xdb88c)]
unsafe fn affinity(r: u32) -> i32 {
    source(r).map_or_else(|| original!(r), |s| i32::from(s.level.affinity))
}
#[forge::hook(offset = 0xdb900)]
unsafe fn sharpness_level(r: u32) -> u32 {
    source(r).map_or_else(|| original!(r), |s| u32::from(s.level.sharpness_level))
}
#[forge::hook(offset = 0xdbe74)]
unsafe fn defense(r: u32) -> u32 {
    source(r).map_or_else(|| original!(r), |s| u32::from(s.level.defense))
}
#[forge::hook(offset = 0xdb984)]
unsafe fn element(r: u32, hand: usize) -> u32 {
    source(r).map_or_else(
        || original!(r, hand),
        |s| {
            s.equipment
                .equipment
                .elements
                .get(hand)
                .map_or(0, |e| e.id().into())
        },
    )
}
#[forge::hook(offset = 0xdba5c)]
unsafe fn magnitude(r: u32, hand: usize) -> i32 {
    source(r).map_or_else(
        || original!(r, hand),
        |s| {
            s.level
                .element_values
                .get(hand)
                .copied()
                .unwrap_or(0)
                .into()
        },
    )
}

fn equipped_source(record: u32) -> Option<Source> {
    if let Some(source) = source(record) {
        return Some(source);
    }
    let (item, _) = inventory::selected()?;
    let (current, actor) = unsafe {
        (
            inventory::current_equipment(),
            inventory::actor_equipment(crate::weapon::local_hunter()),
        )
    };
    if ![current, actor]
        .into_iter()
        .any(|eq| eq != 0 && eq + 24 == record)
    {
        return None;
    }
    let (_, equipment) = inventory::definition(item.equipment.get())?;
    Some(Source {
        record,
        equipment,
        level: equipment.levels[usize::from(item.upgrade)],
    })
}
#[forge::hook(offset = 0xde16c)]
unsafe fn special(record: u32) -> u32 {
    if let Some(s) = equipped_source(record) {
        return s.equipment.equipment.special_skill.into();
    }
    original!(record)
}
#[forge::hook(offset = 0xde1c0)]
unsafe fn frenzy_affinity(record: u32) -> bool {
    if let Some(s) = equipped_source(record) {
        return s.equipment.equipment.special_skill == 3;
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
        && let Some((item, _)) = inventory::selected()
        && actor == unsafe { crate::weapon::local_hunter() }
        && let Some((_, e)) = inventory::definition(item.equipment.get())
        && let Some(curve) = sharpness_curve(e.levels[usize::from(item.upgrade)].sharpness_type)
    {
        return curve[..6].partition_point(|&limit| u32::from(limit) < remaining) as u32;
    }
    original!(actor, remaining)
}
pub(super) fn install(base: u32) {
    forge::install_hook!(base, special);
    forge::install_hook!(base, frenzy_affinity);
    forge::install_hook!(base, sharpness_grade);
    forge::install_hook!(base, calculate);
    forge::install_hook!(base, attack);
    forge::install_hook!(base, affinity);
    forge::install_hook!(base, sharpness_level);
    forge::install_hook!(base, defense);
    forge::install_hook!(base, element);
    forge::install_hook!(base, magnitude);
}
