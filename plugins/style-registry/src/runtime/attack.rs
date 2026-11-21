use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use mhgu_sdk::style::action::{self, Attack, melee::BLADE_CONTACT_OFFSETS};

struct HitData {
    actor: u32,
    index: u32,
    row: [u32; 14],
}

static HIT_DATA: AtomicPtr<HitData> = AtomicPtr::new(ptr::null_mut());

#[forge::hook(offset = 0x2a8a88)]
unsafe fn weapon_row(actor: u32, index: u32) -> u32 {
    if let Some(data) = unsafe { HIT_DATA.load(Ordering::Acquire).as_ref() }
        && (data.actor, data.index) == (actor, index)
    {
        return data.row.as_ptr() as u32;
    }
    original!(actor, index)
}

pub(super) unsafe extern "C" fn submit(actor: u32, attack: *const Attack) -> u32 {
    unsafe {
        let Some(attack) = attack.as_ref() else {
            return 0;
        };
        let Some(style) = super::playback_style(actor) else {
            return 0;
        };
        let (group, slot) = action::current(actor);
        if !action::custom(group, slot) {
            return 0;
        }
        let hit_shape = (actor + 12700) as *mut u32;
        let shape = hit_shape.read();
        if shape == 0 {
            return 0;
        }
        let index = u32::from(attack.template);
        let base = forge::mem::text_addr();
        let lookup: unsafe extern "C" fn(u32, u32) -> *const [u32; 14] =
            core::mem::transmute(base + 0x2a8a88);
        let Some(original) = lookup(actor, index).as_ref() else {
            return 0;
        };
        let mut data = HitData {
            actor,
            index,
            row: *original,
        };
        // The event has already reached the registered motion frame.
        // Feed power to the constructor before GU applies weapon modifiers.
        data.row[0] = 1 << 16;
        data.row[1] = (data.row[1] & 0xffff0000) | u32::from(attack.power);
        if attack.flags & Attack::NO_ATTACKER_HITSTOP != 0 {
            // GU 168960 copies HDP byte 37 to contact +108. 16DB68
            // uses it for both the attacker's slow and full-stop timers.
            data.row[9] &= !(0xff << 8);
        }
        if attack.flags & Attack::NO_MOUNT != 0 {
            // HDP offsets 8, 12 and 51 feed the hit type, flags and buildup.
            if matches!(data.row[2] as u8, 34..=36 | 73) {
                data.row[2] = (data.row[2] & !0xff) | 1;
            }
            data.row[3] &= 0x7fff_ffff;
            data.row[12] &= 0x00ff_ffff;
        }
        if attack.shape != 0 {
            data.row[8] &= !0xff;
            hit_shape.write(attack.shape);
        }
        let previous = HIT_DATA.swap(ptr::from_mut(&mut data), Ordering::AcqRel);
        let hand = usize::from(attack.flags & Attack::SECONDARY != 0);
        let hit = actor + BLADE_CONTACT_OFFSETS[hand];
        let mut flags = attack.flags
            & !(Attack::DEFLECTABLE
                | Attack::SECONDARY
                | Attack::NO_MOUNT
                | Attack::NO_ATTACKER_HITSTOP);
        if ((actor + 10057) as *const u8).read() & 8 != 0 {
            flags |= 0x10;
        }
        super::weapon::strike(actor, style, flags, |flags| {
            // 2B7240 supplies a fixed flags-table row to the same constructor
            // and weapon virtual. A style attack can specify its own flags.
            let construct: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32) -> u32 =
                core::mem::transmute(base + 0x2b72d4);
            construct(actor, actor, actor, 0, hit, 1, index, flags);
            HIT_DATA.store(previous, Ordering::Release);
            hit_shape.write(shape);

            ((hit + 92) as *mut f32).write(attack.delay);
            ((hit + 96) as *mut f32).write(attack.duration);
            if attack.no_sharpness_cost != 0 {
                let hit_flags = (hit + 48) as *mut u32;
                hit_flags.write(hit_flags.read() | 0x08000000);
            }
            let vtable = (actor as *const *const u32).read();
            let adjust = vtable.add(1992 / 4).read();
            if adjust != 0 {
                let adjust: unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32 =
                    core::mem::transmute(adjust);
                adjust(actor, hit, 1, index, flags);
            }
        });
        // Recoil decisions belong to the sender. A remote swing must not
        // displace a pending hit in the local hunter's deflection window.
        if super::local_hunter() == actor {
            let serial = (attack.flags & Attack::DEFLECTABLE != 0)
                .then(|| super::deflection::contact_serial(hit));
            super::deflection::submitted(actor, slot, hand, serial);
        }
        1
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, weapon_row);
}
