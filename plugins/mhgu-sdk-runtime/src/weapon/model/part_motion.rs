use super::{equipment, read, set};
use crate::weapon::{gu, inventory, resource, style};
use core::sync::atomic::Ordering::{Acquire, Release};
use mhgu_sdk::weapon::style::part_motion::{self, Motions, Pose};

static SERVICE: part_motion::Service = part_motion::Service(bind);

unsafe extern "C" fn bind(id: u32, motions: *const Motions) -> bool {
    let Some(entry) = style::registered_entry(id).filter(|entry| entry.kind.is_some()) else {
        return false;
    };
    let Some(motions) = (unsafe { motions.as_ref() }) else {
        return false;
    };
    if !(1..=8).contains(&motions.banks.len())
        || motions
            .banks
            .iter()
            .any(|path| !(1..64).contains(&path.to_bytes().len()))
    {
        return false;
    }
    entry
        .part_motion
        .store(core::ptr::from_ref(motions).cast_mut(), Release);
    true
}

pub(super) fn registered(equipment: inventory::RegisteredEquipment) -> Option<&'static Motions> {
    let (entry, _) = inventory::weapon_type(equipment.kind.id.get())?;
    unsafe { entry.part_motion.load(Acquire).as_ref() }
}

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global mhgu_part_track",
    ".hidden mhgu_part_track",
    ".type mhgu_part_track,%function",
    "mhgu_part_track:",
    "vmov s0,r3",
    "ldr ip,[sp]",
    "vmov s1,ip",
    "vmov.f32 s2,#1.0",
    "mov r3,#0",
    "ldr ip,[sp,#4]",
    "bx ip",
);

unsafe extern "C" {
    fn mhgu_part_track(part: u32, layer: u32, clip: u32, blend: u32, frame: u32, entry: u32);
}

unsafe fn track(part: u32, actor: u32) -> Option<()> {
    unsafe {
        let manager = read(forge::mem::text_addr() + 0x1885b58);
        if manager == 0
            || actor == 0
            || read(part + 48) != actor
            || read(actor + 9120) != part
            || read(part + 240) == 0
            || !mhgu_sdk::service::on_game_thread()
        {
            return None;
        }
        let motions = registered(equipment(manager, actor)?)?;
        let motion = gu!(0x53a058, actor);
        let vtable = read(actor);
        let drawn: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(read(vtable + 376));
        let clip = (motions.select)(Pose {
            actor,
            motion,
            drawn: drawn(actor) == 1,
        });
        let path = *motions.banks.get(usize::from(clip.bank))?;
        let resource = resource::load(0x2125210, path)?;
        let slot = u32::from(clip.slot);
        let row = gu!(0xb49e54, resource, slot);
        if row == 0 {
            return None;
        }
        let bank = (8..16)
            .rev()
            .find(|&bank| read(part + 3844 + 4 * bank) == resource)
            .or_else(|| {
                (8..16)
                    .rev()
                    .find(|&bank| read(part + 3844 + 4 * bank) == 0)
            })?;
        if read(part + 3844 + 4 * bank) == 0 {
            gu!(0x957400, part, resource, bank);
        }
        let frames = read(row + 8) as f32;
        let loop_frame = read(row + 12) as f32;
        let frame = f32::from_bits(read(actor + 1280)).max(0.);
        let frame = if frame >= frames && loop_frame < frames {
            loop_frame + (frame - loop_frame) % (frames - loop_frame)
        } else {
            frame.min((frames - 1.).max(0.))
        };
        let blend = f32::from_bits(read(actor + 1268)).max(0.);
        let packed = (bank << 8) | slot;
        let play = forge::mem::text_addr() + 0x954640;
        // GU 308B3C feeds the part from the hunter playhead before 93AF60.
        set(part, 5060, motion);
        let current = (part + 1204) as *const u16;
        if u32::from(current.read()) != packed {
            mhgu_part_track(part, 0, packed, blend.to_bits(), frame.to_bits(), play);
        }
        if (current.add(160)).read() != u16::MAX {
            mhgu_part_track(part, 1, 0xffff, 0, 0, play);
        }
        set(part, 1276, frame);
        set(part, 1212, read(part + 1212) | 0x10);
        Some(())
    }
}

#[forge::hook(offset = 0x3078b8)]
unsafe fn update(part: u32, actor: u32, kind: u32) -> u32 {
    let result = original!(part, actor, kind);
    unsafe { track(part, actor) };
    result
}

pub(super) fn install(base: u32) {
    if unsafe { part_motion::publish(&SERVICE) } {
        forge::install_hook!(base, update);
    }
}
