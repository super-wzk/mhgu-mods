use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use mhgu_sdk::{
    combat::collision::hurt::MonsterSpawn,
    service,
    sync::Shared,
    weapon::style::{
        action::{
            self, Attack, hitstop,
            melee::{
                self, BLADE_CONTACT_OFFSETS, MonsterReaction, MonsterStrikes, Strike,
                StrikeContact, StrikePower, WorldStrikePower,
            },
        },
        remote::{PLAYER_COUNT, PeerStates, Roster},
    },
};

mod reaction;

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
    reaction: MonsterReaction,
}

struct MeleeRow {
    row: [u32; 14],
    power: StrikePower,
    reaction: MonsterReaction,
    hitstop: u8,
}

static LOCAL_STRIKES: Shared<[Option<RegisteredStrike>; 2]> = Shared::new([None; 2]);
static PEER_STRIKES: PeerStates<[Option<RegisteredStrike>; 2]> = PeerStates::new();

pub(super) fn begin_hunt() {
    LOCAL_STRIKES.with(|strikes| *strikes = [None; 2]);
    for index in 0..PLAYER_COUNT {
        PEER_STRIKES.update(index, 0, |_, _| {});
    }
    for entry in super::registered_entries() {
        if let Some(observer) = unsafe { entry.monster_strikes.load(Ordering::Acquire).as_ref() } {
            (observer.begin_hunt)();
        }
    }
}

pub(super) fn clear(actor: u32) {
    reaction::clear(actor);
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

extern "C" fn bind_monster(id: u32, observer: &'static MonsterStrikes) -> bool {
    let Some(entry) = super::registered_entry(id).filter(|entry| entry.style.independent) else {
        return false;
    };
    entry
        .monster_strikes
        .store(ptr::from_ref(observer).cast_mut(), Ordering::Release);
    true
}

extern "C" fn bind_world(id: u32, callback: WorldStrikePower) -> bool {
    let Some(entry) = super::registered_entry(id).filter(|entry| entry.style.independent) else {
        return false;
    };
    entry
        .world_strike_power
        .store(callback as *mut (), Ordering::Release);
    true
}

unsafe fn recorded_strike(
    actor: u32,
    hit: u32,
    hand: usize,
    local: bool,
) -> Option<RegisteredStrike> {
    let mut found = None;
    if local {
        found = LOCAL_STRIKES.with(|strikes| strikes[hand]);
    } else if let Some(index) = unsafe { Roster::current() }.and_then(|r| r.peer_index(actor)) {
        PEER_STRIKES.update(index, actor, |strikes, _| {
            found = strikes.as_ref().and_then(|strikes| strikes[hand]);
        });
    }
    found.filter(|s| {
        s.actor == actor && s.serial == unsafe { super::deflection::contact_serial(hit) }
    })
}

// Only a recorded restraint finisher gets the target/row override.
unsafe fn finisher_owner(hit: u32) -> Option<(u32, bool, u32)> {
    let actor = unsafe { crate::weapon::read::<u32>(hit, 12) };
    let hand = BLADE_CONTACT_OFFSETS
        .iter()
        .position(|&n| hit.wrapping_sub(actor) == n)?;
    let local = unsafe { super::local_hunter() } == actor;
    let strike = unsafe { recorded_strike(actor, hit, hand, local) }?;
    (strike.reaction == MonsterReaction::Topple).then_some((actor, local, strike.style))
}

#[forge::hook(offset = 0x16c0c0)]
unsafe fn intersect_monster(manager: u32, hit: u32, target: u32) -> u32 {
    use crate::weapon::{read, set};
    use melee::RestrainedContact;
    let Some((actor, local, style_id)) = (unsafe { finisher_owner(hit) }) else {
        return original!(manager, hit, target);
    };
    unsafe {
        let entry = if local {
            crate::weapon::style::local_entry(actor)
        } else {
            crate::weapon::style::multiplayer::peer_entry(actor)
        };
        let Some(observer) = entry
            .filter(|e| e.style.id == style_id)
            .and_then(|e| e.monster_strikes.load(Ordering::Acquire).as_ref())
        else {
            return 0;
        };
        let Some(spawn) = MonsterSpawn::from_owned_damage_target(actor, target) else {
            return 0;
        };
        let row = match (observer.restrained_contact)(actor, &spawn) {
            RestrainedContact::Reject => return 0,
            RestrainedContact::AnyPart => None,
            RestrainedContact::Row(row) => Some(row),
        };
        let index = read::<u32>(hit, 28);
        if index >= 64 {
            return 0;
        }
        let sphere = manager + 8240 + index * 32;
        if read::<u8>(sphere, 0) != 1 {
            return 0;
        }
        // FZ 816FE7F4..816FE840 accepts the caster's captive at the marked
        // hurt row regardless of distance. Let GU settle the resulting hit.
        mhgu_sdk::combat::collision::hurt::with_restrained_row(manager, target, row, |point| {
            let center = read::<[f32; 3]>(sphere, 8);
            set(sphere, 8, point);
            let result = original!(manager, hit, target);
            set(sphere, 8, center);
            result
        })
        .unwrap_or(0)
    }
}

unsafe fn notify_contact(actor: u32, hit: u32, target: u32, affinity: *const f32) {
    if target == 0 {
        return;
    }
    let local = unsafe { super::local_hunter() } == actor;
    let entry = unsafe {
        if local {
            super::local_entry(actor)
        } else {
            super::multiplayer::peer_entry(actor)
        }
    };
    let Some(entry) = entry else {
        return;
    };
    let callback = if local {
        entry.strike_contact.load(Ordering::Acquire)
    } else {
        ptr::null_mut()
    };
    let observer = unsafe { entry.monster_strikes.load(Ordering::Acquire).as_ref() };
    let (power, reaction) = if let Some(hand) = BLADE_CONTACT_OFFSETS
        .iter()
        .position(|&offset| offset == hit.wrapping_sub(actor))
    {
        let Some(strike) = (unsafe { recorded_strike(actor, hit, hand, local) })
            .filter(|s| s.style == entry.style.id)
        else {
            return;
        };
        (strike.power, strike.reaction)
    } else {
        let lookup = entry.world_strike_power.load(Ordering::Acquire);
        if (callback.is_null() && observer.is_none()) || lookup.is_null() {
            return;
        }
        let lookup: WorldStrikePower = unsafe { core::mem::transmute(lookup) };
        let mut power = StrikePower {
            motion: 0,
            bonus: 0,
        };
        if !lookup(actor, hit, &mut power) {
            return;
        }
        (power, MonsterReaction::Native)
    };
    if (observer.is_some() || reaction != MonsterReaction::Native)
        && service::on_game_thread()
        && let Some(spawn) = unsafe { MonsterSpawn::from_owned_damage_target(actor, target) }
    {
        if let Some(observer) = observer {
            (observer.contact)(actor, &spawn, power, unsafe { affinity.read() });
        }
        if reaction == MonsterReaction::Topple {
            reaction::queue(actor, entry.style.id, target, spawn);
        }
    }
    if !callback.is_null() {
        let spawn = unsafe { MonsterSpawn::from_damage_target(actor, target) };
        let notify: StrikeContact = unsafe { core::mem::transmute(callback) };
        // All three GU damage paths initialize this multiplier to 1.0.
        notify(actor, power, unsafe { affinity.read() }, spawn.as_ref());
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

#[forge::hook(offset = 0x16db68)]
unsafe fn resolve_hitstop(target: u32, hit: u32) -> u32 {
    unsafe {
        let value = (hit + 108) as *mut u8;
        let encoded = value.read();
        let Some(frames) = hitstop::frames(encoded) else {
            return original!(target, hit);
        };
        let actor = ((hit + 112) as *const u32).read();
        let eligible = actor != 0
            && ((hit + 110) as *const u8).read() == 0
            && ((hit + 48) as *const u8).read() & 8 != 0
            && mhgu_sdk::weapon::style::hunter::is_quest(actor);
        value.write(hitstop::native_factor(encoded));
        let result = original!(target, hit);
        value.write(encoded);
        if eligible {
            let full = (actor + 10156) as *mut f32;
            let slow = (actor + 10160) as *mut f32;
            if full.read() > 0. || slow.read() > 0. {
                // FZ 81820E3A holds the hunter's playhead for every SDT stop tick.
                full.write(frames);
                slow.write(0.);
            }
        }
        result
    }
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
    let power = strike.power.adjusted(1.);
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
        row: strike.hdp(),
        power: strike.power,
        reaction: strike.reaction,
        hitstop: strike.hitstop,
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
            BLADE_CONTACT_OFFSETS
                .iter()
                .position(|offset| {
                    let hit = actor + offset;
                    ((hit + 5) as *const u8).read() == 0
                        || ((hit + 96) as *const f32).read() <= 0.
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
                // retains the authored time, as in Attack::submit_with_hitstop.
                ((hit + 108) as *mut u8).write(hitstop::contact_value(melee.hitstop));
                // 169548 reads the computed melee attack only with bit 0x20.
                // Set it after 2B76CC, without enabling retail weapon bonuses.
                let hit_flags = (hit + 48) as *mut u32;
                // FZ family 120/121 events spawn with flags zero. Their scene
                // lifetime continues across play-rate and animation changes.
                hit_flags.write(hit_flags.read() & !(0x800 | 0x300) | 0x20);
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
        let recorded = registered.map(|melee| RegisteredStrike {
            actor,
            style: style.id,
            serial: super::deflection::contact_serial(hit),
            power: melee.power,
            reaction: melee.reaction,
        });
        // Recoil decisions belong to the sender. Peer receipts are only for
        // the console currently simulating the hit monster.
        if super::local_hunter() == actor {
            LOCAL_STRIKES.with(|strikes| strikes[hand] = recorded);
            let serial = (attack.flags & Attack::DEFLECTABLE != 0)
                .then(|| super::deflection::contact_serial(hit));
            super::deflection::submitted(actor, slot, hand, serial);
        } else if let Some(index) = Roster::current().and_then(|r| r.peer_index(actor)) {
            PEER_STRIKES.update(index, actor, |strikes, _| {
                strikes.get_or_insert([None; 2])[hand] = recorded;
            });
        }
        1
    }
}

pub(super) fn install(base: u32) {
    reaction::install(base);
    forge::install_hook!(base, intersect_monster);
    use mhgu_sdk::weapon::style::action::stun;
    static SERVICE: stun::Service = stun::Service(submit_stun);
    static MELEE: melee::StrikeService = melee::StrikeService {
        submit: submit_melee,
        bind: bind_contact,
        bind_world,
        bind_monster,
    };
    unsafe { stun::publish(&SERVICE) };
    unsafe { melee::publish_strike(&MELEE) };
    forge::install_hook!(base, weapon_row);
    forge::install_hook!(base, resolve_affinity);
    forge::install_hook!(base, resolve_hitstop);
    unsafe { hitstop::publish() };
}
