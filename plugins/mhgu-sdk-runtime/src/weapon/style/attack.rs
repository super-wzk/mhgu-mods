use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::action::{
        self, Attack,
        melee::{self, BLADE_CONTACT_OFFSETS, Strike, StrikeContact, StrikePower},
    },
};

struct HitData {
    actor: u32,
    index: u32,
    row: [u32; 14],
}

static HIT_DATA: AtomicPtr<HitData> = AtomicPtr::new(ptr::null_mut());

#[derive(Clone, Copy)]
struct RegisteredStrike {
    actor: u32,
    style: u32,
    serial: u16,
    power: StrikePower,
}

struct MeleeRow {
    row: [u32; 14],
    power: StrikePower,
}

static LOCAL_STRIKES: Shared<[Option<RegisteredStrike>; 2]> = Shared::new([None; 2]);

pub(super) fn clear(actor: u32) {
    LOCAL_STRIKES.with(|strikes| {
        for strike in strikes {
            strike.take_if(|contact| contact.actor == actor);
        }
    });
}

extern "C" fn bind_contact(id: u32, callback: StrikeContact) -> bool {
    let Some(entry) = super::registered_entry(id).filter(|entry| entry.style.independent) else {
        return false;
    };
    entry
        .strike_contact
        .store(callback as *mut (), Ordering::Release);
    true
}

unsafe fn notify_contact(actor: u32, hit: u32, target: u32, affinity: *const f32) {
    if actor == 0 || target == 0 || actor != unsafe { super::local_hunter() } {
        return;
    }
    let Some(hand) = BLADE_CONTACT_OFFSETS
        .iter()
        .position(|&offset| offset == hit.wrapping_sub(actor))
    else {
        return;
    };
    let strike = LOCAL_STRIKES.with(|strikes| strikes[hand]);
    let Some(strike) = strike.filter(|strike| {
        strike.actor == actor && strike.serial == unsafe { super::deflection::contact_serial(hit) }
    }) else {
        return;
    };
    let Some(entry) =
        (unsafe { super::local_entry(actor) }).filter(|entry| entry.style.id == strike.style)
    else {
        return;
    };
    let callback = entry.strike_contact.load(Ordering::Acquire);
    if !callback.is_null() {
        let notify: StrikeContact = unsafe { core::mem::transmute(callback) };
        // All three GU damage paths initialize this multiplier to 1.0.
        notify(actor, strike.power, unsafe { affinity.read() });
    }
}

#[forge::hook(offset = 0x17885c)]
unsafe fn resolve_affinity(
    manager: u32,
    hit: u32,
    target: u32,
    part: u32,
    actor: u32,
    affinity: *mut f32,
) -> u32 {
    let result = original!(manager, hit, target, part, actor, affinity);
    unsafe { notify_contact(actor, hit, target, affinity) };
    result
}

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
    unsafe { submit_row(actor, attack, None, None) }
}

unsafe extern "C" fn submit_stun(actor: u32, attack: *const Attack, stun: u8) -> u32 {
    unsafe { submit_row(actor, attack, Some(stun), None) }
}

unsafe extern "C" fn submit_melee(actor: u32, strike: *const Strike, shape: u32) -> u32 {
    let Some(strike) = (unsafe { strike.as_ref() }) else {
        return 0;
    };
    if shape == 0
        || !strike.delay.is_finite()
        || strike.delay < 0.
        || !strike.duration.is_finite()
        || strike.duration <= 0.
    {
        return 0;
    }
    // GU's 56-byte HDP data for a basic player melee contact.
    let mut row = [0u32; 14];
    row[2] = 1 | (30 << 16);
    row[3] = strike.damage as u32;
    row[4] = 2;
    row[8] = 255 << 24; // No repeating hit interval.
    row[9] = 255 | (u32::from(strike.hitstop) << 8);
    row[10] = 1f32.to_bits();
    row[11] = 1f32.to_bits();
    let power = strike.power.adjusted(1.);
    row[12] = u32::from(power);
    // Bits 1 and 2 apply weapon attributes and critical hits. Bit 0
    // also applies retail spirit and shield-strengthening modifiers.
    let mut flags = (Attack::WEAPON_MODIFIERS & !1) | Attack::PUNISHING_DRAW;
    if strike.deflectable {
        flags |= Attack::DEFLECTABLE;
    }
    let attack = Attack {
        template: 0,
        power,
        delay: strike.delay,
        duration: strike.duration,
        flags,
        shape,
        no_sharpness_cost: 0,
    };
    let stun = (strike.stun != 0).then_some(strike.stun);
    let melee = MeleeRow {
        row,
        power: strike.power,
    };
    unsafe { submit_row(actor, &attack, stun, Some(melee)) }
}

unsafe fn submit_row(
    actor: u32,
    attack: *const Attack,
    stun: Option<u8>,
    registered: Option<MeleeRow>,
) -> u32 {
    unsafe {
        let Some(attack) = attack.as_ref() else {
            return 0;
        };
        // The public strike API accepts identities. Validate membership
        // against the live local hunter or peer roster before reading actor.
        let Some(style) = super::playback_style(actor) else {
            return 0;
        };
        let (group, slot) = action::current(actor);
        if !action::custom(group, slot) || registered.is_some() && !style.independent {
            return 0;
        }
        let hit_shape = (actor + 12700) as *mut u32;
        let shape = hit_shape.read();
        if shape == 0 && registered.is_none() {
            return 0;
        }
        let index = u32::from(attack.template);
        let base = forge::mem::text_addr();
        let row = if let Some(melee) = &registered {
            melee.row
        } else {
            let lookup: unsafe extern "C" fn(u32, u32) -> *const [u32; 14] =
                core::mem::transmute(base + 0x2a8a88);
            let Some(original) = lookup(actor, index).as_ref() else {
                return 0;
            };
            *original
        };
        let mut data = HitData { actor, index, row };
        // The event has already reached the registered motion frame.
        // Feed power to the constructor before GU applies weapon modifiers.
        data.row[0] = 1 << 16;
        data.row[1] = (data.row[1] & 0xffff0000) | u32::from(attack.power);
        let draw = ((actor + 10057) as *const u8).read() & 8 != 0;
        if let Some(stun) = stun {
            // 168960 copies the signed HDP byte 34 into the base KO field.
            data.row[8] = (data.row[8] & !(255 << 16)) | (u32::from(stun.min(127)) << 16);
        } else if draw
            && attack.flags & Attack::PUNISHING_DRAW != 0
            && (data.row[8] >> 16) as u8 == 0
        {
            // GU 2B76CC reads the signed HDP KO field when applying Punishing Draw.
            data.row[8] |= u32::from(Attack::draw_ko(attack.flags) as u8) << 16;
        }
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
        let hand = if registered.is_some() {
            let decode: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(base + 0x53a058);
            let motion = decode(actor);
            BLADE_CONTACT_OFFSETS
                .iter()
                .position(|offset| {
                    let hit = actor + offset;
                    ((hit + 5) as *const u8).read() == 0
                        || ((hit + 96) as *const f32).read() <= 0.
                        || ((hit + 116) as *const u32).read() != motion
                })
                .unwrap_or(0)
        } else {
            usize::from(attack.flags & Attack::SECONDARY != 0)
        };
        let hit = actor + BLADE_CONTACT_OFFSETS[hand];
        let draw_ko_flags = if attack.flags & Attack::PUNISHING_DRAW != 0 {
            Attack::DRAW_KO_MASK
        } else {
            0
        };
        let mut flags = attack.flags
            & !(Attack::DEFLECTABLE
                | draw_ko_flags
                | Attack::SECONDARY
                | Attack::NO_MOUNT
                | Attack::NO_ATTACKER_HITSTOP);
        if draw {
            flags |= 0x10;
        }
        let previous = HIT_DATA.swap(ptr::from_mut(&mut data), Ordering::AcqRel);
        super::routes::strike(actor, style, flags, |flags| {
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
            if let Some(melee) = &registered {
                // 168960 treats factor 1 as zero. Explicit Frontier hitstop
                // retains the authored factor, as in Attack::submit_with_hitstop.
                ((hit + 108) as *mut u8).write((melee.row[9] >> 8) as u8);
                // 169548 reads the computed melee attack only with bit 0x20.
                // Set it after 2B76CC, without enabling retail weapon bonuses.
                let hit_flags = (hit + 48) as *mut u32;
                hit_flags.write(hit_flags.read() & !0x800 | 0x20);
            } else {
                let vtable = (actor as *const *const u32).read();
                let adjust = vtable.add(1992 / 4).read();
                if adjust != 0 {
                    let adjust: unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32 =
                        core::mem::transmute(adjust);
                    adjust(actor, hit, 1, index, flags);
                }
            }
        });
        // Recoil decisions belong to the sender. A remote swing must not
        // displace a pending hit in the local hunter's deflection window.
        if super::local_hunter() == actor {
            LOCAL_STRIKES.with(|strikes| {
                strikes[hand] = registered.map(|melee| RegisteredStrike {
                    actor,
                    style: style.id,
                    serial: super::deflection::contact_serial(hit),
                    power: melee.power,
                });
            });
            let serial = (attack.flags & Attack::DEFLECTABLE != 0)
                .then(|| super::deflection::contact_serial(hit));
            super::deflection::submitted(actor, slot, hand, serial);
        }
        1
    }
}

pub(super) fn install(base: u32) {
    use mhgu_sdk::weapon::style::action::stun;
    static SERVICE: stun::Service = stun::Service(submit_stun);
    static MELEE: melee::StrikeService = melee::StrikeService {
        submit: submit_melee,
        bind: bind_contact,
    };
    unsafe { stun::publish(&SERVICE) };
    unsafe { melee::publish_strike(&MELEE) };
    forge::install_hook!(base, weapon_row);
    forge::install_hook!(base, resolve_affinity);
}
