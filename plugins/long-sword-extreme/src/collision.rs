//! GU contact setup for Frontier long sword attacks.
use crate::{native, read, write};
use mhgu_sdk::combat::collision::{CachedHitShape, Contact};

pub(super) const BLADE_TEMPLATE: u16 = 1;

pub(super) static BLADES: [CachedHitShape; 3] = [
    CachedHitShape::new(cr"player\hit\frontier_ls\blade_0"),
    CachedHitShape::new(cr"player\hit\frontier_ls\blade_32"),
    CachedHitShape::new(cr"player\hit\frontier_ls\blade_33"),
];
pub(super) static DISCHARGE: CachedHitShape =
    CachedHitShape::new(cr"player\hit\frontier_ls\discharge");
pub(super) static INSTANT: CachedHitShape = CachedHitShape::new(cr"player\hit\frontier_ls\instant");
pub(super) static RELEASE: CachedHitShape = CachedHitShape::new(cr"player\hit\frontier_ls\release");

pub(super) unsafe fn preload() {
    for shape in BLADES.iter().chain([&DISCHARGE, &INSTANT, &RELEASE]) {
        unsafe { shape.preload() };
    }
}

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
        contact.prepare_world_attack(actor, &hdp, shape);
        clear_native_spirit(contact.as_raw());
    }
}

unsafe extern "C" fn collision_pass(manager: u32) {
    unsafe {
        for index in 0..crate::peer::COUNT {
            let slot = crate::peer::Slot(index);
            crate::peer::clear_interrupted_effects(slot);
            crate::instant::collision_pass(slot, manager);
            crate::discharge::submit(slot, manager);
            crate::release::collision_pass(slot, manager);
        }
    }
}

pub(super) fn install() {
    unsafe { mhgu_sdk::style::world_contact::bind_pass(crate::STYLE_ID, collision_pass) };
}
