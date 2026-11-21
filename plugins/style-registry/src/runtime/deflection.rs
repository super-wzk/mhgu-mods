//! Match native blade contacts to a registered style's deflection window.
use mhgu_sdk::{
    style::{
        action::{self, melee},
        rules::Deflection,
    },
    sync::Shared,
};

struct Window {
    actor: u32,
    slot: u16,
    serials: [Option<u16>; 2],
    deflected: bool,
}

impl Window {
    fn matches(&self, actor: u32, current: (u16, u16)) -> bool {
        self.actor == actor && current == (action::GROUP, self.slot)
    }
}

static WINDOW: Shared<Option<Window>> = Shared::new(None);
static API: melee::DeflectionService = melee::DeflectionService(take);

pub(super) unsafe fn contact_serial(hit: u32) -> u16 {
    unsafe { *((hit + 52) as *const u16) }
}

pub(super) fn clear(actor: u32) {
    WINDOW.with(|window| {
        window.take_if(|active| active.actor == actor);
    });
}

pub(super) fn submitted(actor: u32, slot: u16, hand: usize, serial: Option<u16>) {
    WINDOW.with(|window| {
        let Some(serial) = serial else {
            if let Some(active) = window.as_mut().filter(|active| active.actor == actor) {
                active.serials[hand] = None;
            }
            return;
        };
        let active = match window {
            Some(active) if (active.actor, active.slot) == (actor, slot) => active,
            _ => window.insert(Window {
                actor,
                slot,
                serials: [None; 2],
                deflected: false,
            }),
        };
        active.serials[hand] = Some(serial);
    });
}

unsafe extern "C" fn take(actor: u32) -> bool {
    let current = unsafe { action::current(actor) };
    WINDOW.with(|window| {
        window
            .take_if(|active| active.matches(actor, current) && active.deflected)
            .is_some()
    })
}

unsafe fn record(actor: u32, hit: u32, target: u32, part: u32, ratio: *const f32) {
    let offset = hit.wrapping_sub(actor);
    let Some(hand) = melee::BLADE_CONTACT_OFFSETS
        .iter()
        .position(|&blade| blade == offset)
    else {
        return;
    };
    if target == 0 {
        return;
    }
    let Some(policy) = unsafe { super::local_entry(actor) }
        .and_then(|entry| entry.rules())
        .and_then(|rules| rules.deflection)
    else {
        return;
    };
    let (current, serial) = unsafe { (action::current(actor), contact_serial(hit)) };
    let response = WINDOW.with(|window| {
        let active = window.as_mut()?;
        if !active.matches(actor, current) || active.serials[hand] != Some(serial) {
            return None;
        }
        let factor = unsafe {
            if *((part + 10) as *const u16) & 0x2000 != 0 {
                0.
            } else {
                ratio.read()
            }
        };
        let result = policy(factor);
        if result != Deflection::KeepWindow {
            active.serials = [None; 2];
        }
        Some(result)
    });
    // Run GU's equipment and status callbacks outside the lock, then
    // recheck ownership before publishing the result.
    if response == Some(Deflection::Recoil) && !unsafe { ignores_deflection(actor, hit) } {
        WINDOW.with(|window| {
            if let Some(active) = window
                .as_mut()
                .filter(|active| active.matches(actor, current))
            {
                active.deflected = true;
            }
        });
    }
}

// GU 2A87A8 normally takes the stronger grade across the hunter's hands.
// Evaluate its grade-zero exemptions for the contact that invoked the policy.
unsafe fn ignores_deflection(actor: u32, hit: u32) -> bool {
    unsafe {
        let base = forge::mem::text_addr();
        let flags = *((hit + 48) as *const u32);
        let checks_skills = flags & 0x40000 == 0;
        let has: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(base + 0x2864c4);
        if flags & 0x08000000 != 0
            || (checks_skills
                && [87, 303, 278, 296]
                    .into_iter()
                    .any(|id| has(actor, id) != 0))
        {
            return true;
        }
        let vtable = *(actor as *const *const usize);
        let status: unsafe extern "C" fn(u32, u64) -> u32 =
            core::mem::transmute(*vtable.add(440 / 4));
        let active: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(*vtable.add(520 / 4));
        if (checks_skills && active(actor, 1) == 1 && *((actor + 10716) as *const u8) > 1)
            || status(actor, 8) != 0
            || status(actor, 14u64 << 32) != 0
        {
            return true;
        }
        let weapon: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(base + 0xdf814);
        let has_effect: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(base + 0xdd4c4);
        let applies: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(*vtable.add(536 / 4));
        let eq = *((actor + 5060) as *const u32);
        has_effect(weapon(eq + 4, 0), 22) == 1 && applies(actor, 2) != 0
    }
}

#[forge::hook(offset = 0x178030)]
unsafe fn hunter(
    manager: u32,
    hit: u32,
    target: u32,
    part: u32,
    point: u32,
    actor: u32,
    factor: *const f32,
    element: u32,
) -> u32 {
    let result = original!(manager, hit, target, part, point, actor, factor, element);
    unsafe { notify_contact(actor, hit, target); record(actor, hit, target, part, factor) };
    result
}

#[forge::hook(offset = 0x1748a8)]
unsafe fn alternate(
    manager: u32,
    hit: u32,
    target: u32,
    part: u32,
    point: u32,
    actor: u32,
    factor: *const f32,
    element: u32,
) -> u32 {
    let result = original!(manager, hit, target, part, point, actor, factor, element);
    unsafe { notify_contact(actor, hit, target); record(actor, hit, target, part, factor) };
    result
}

unsafe extern "C" fn bind_contact(id:u32, callback:melee::ContactCallback){
    if let Some(entry)=super::registered_entry(id){entry.blade_contact.store(callback as u32,core::sync::atomic::Ordering::Release);}
}

unsafe fn notify_contact(actor:u32,hit:u32,target:u32){
    if actor==0 || target==0 || hit.wrapping_sub(actor)!=melee::BLADE_CONTACT_OFFSETS[0] {return;}
    unsafe{
        if let Some(entry)=super::playback_style(actor).and_then(|s| super::registered_entry(s.id)){
            let callback=entry.blade_contact.load(core::sync::atomic::Ordering::Acquire);
            if callback!=0 {
                let call:melee::ContactCallback=core::mem::transmute(callback);
                call(actor);
            }
        }
    }
}

pub(super) fn install(base: u32) {
    static CONTACT:melee::ContactService=melee::ContactService(bind_contact);
    unsafe{melee::publish_contact(&CONTACT);}
    if unsafe { melee::publish_deflection(&API) } {
        forge::install_hook!(base, hunter);
        forge::install_hook!(base, alternate);
    }
}
