//! GU contact setup for Frontier long sword attacks.
use crate::{native, read, write};
use mhgu_sdk::combat::collision::Contact;

pub(super) const BLADE_TEMPLATE: u16 = 1;

pub(super) unsafe fn blade_template(actor: u32) -> Option<u32> {
    let template = unsafe { native!(0x2a8a88, actor, u32::from(BLADE_TEMPLATE)) };
    (template != 0).then_some(template)
}

pub(super) unsafe fn clear_native_spirit(hit: u32) {
    // 168960 copies HDP byte 38 here. 1720D4 would award it in addition
    // to the FZ strike's spirit gain handled by the weapon contact callback.
    unsafe { write(hit, 87, 0u8) };
}

pub(super) unsafe fn setup(
    actor: u32,
    contact: Contact<'_>,
    template: u32,
    shape: u32,
    window: [u16; 2],
    power: u16,
    hitstop: u8,
) {
    unsafe {
        let [start, duration] = window;
        let mut hdp: [u32; 14] = read(template, 0);
        hdp[0] = u32::from(start) | u32::from(duration) << 16;
        hdp[1] = hdp[1] & 0xffff0000 | u32::from(power);
        hdp[2] = hdp[2] & !255 | 1;
        hdp[3] &= 0x7fff_ffff;
        // GU 16DB68 resolves hitstop from the copied HDP byte 37.
        hdp[9] = hdp[9] & !(255 << 8) | u32::from(hitstop) << 8;
        hdp[12] &= 0x00ff_ffff;
        let hit = contact.as_raw();
        // 168960 creates a new collision serial and retains the HTS.
        native!(
            0x168960,
            hit,
            0,
            actor,
            hdp.as_ptr() as u32,
            shape,
            0,
            15,
            1
        );
        // GU 16919C uses bits 0x100 and 0x200 for owner-action expiry and
        // owner-motion time. FZ families 85 and 108 have their own lifetimes.
        write(hit, 48, 0x0a00_0428u32);
        // Apply the equipped weapon's element/status and sharpness modifiers.
        native!(0x2b76cc, actor, hit, hdp.as_ptr() as u32, 7);
        clear_native_spirit(hit);
    }
}
pub(super) unsafe fn enqueue(manager: u32, contact: Contact<'_>) {
    unsafe {
        // The manager's list is intrusive. Guard against the same pass
        // being requested twice before GU clears the list.
        let hit = contact.as_raw();
        let mut linked = read::<u32>(manager, 8224);
        while linked != 0 && linked != hit {
            linked = read(linked, 44);
        }
        if linked == 0 {
            native!(0x1696e4, hit);
        }
    }
}

#[forge::hook(offset = 0x16abac)]
unsafe fn collision_pass(manager: u32) -> u32 {
    unsafe {
        for index in 0..crate::peer::COUNT {
            let slot = crate::peer::Slot(index);
            crate::peer::clear_interrupted_effects(slot);
            crate::instant::collision_pass(slot, manager);
            crate::discharge::submit(slot, manager);
            crate::release::collision_pass(slot, manager);
        }
    }
    original!(manager)
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), collision_pass);
}
