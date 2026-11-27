//! Optional expanded-weapon state on GU's native periodic position packet.
use super::{Entry, local_entry, local_hunter, packet_action, peer_entry, registered_entry};
use crate::weapon::{gu, read, set};
use core::{ptr, sync::atomic::Ordering};
use mhgu_sdk::{
    service::on_game_thread,
    sync::Shared,
    weapon::style::{action, remote},
};

const VTABLE: u32 = 0x17423e4; // GU 29C368 calls 200438 on the periodic position timer.
const POSITION_FLAGS: u32 = 44;
const ACTION_FLAGS: u32 = 16;
// 20758C reflects the position mPlAttr as a u32. GU uses bits 4..7.
const POSITION_PRESENT: u32 = 1 << 27;
const POSITION_STATE_SHIFT: u32 = 24;
const POSITION_ACTION_SHIFT: u32 = 16;
const POSITION_SERIAL_SHIFT: u32 = 8;
const ACTION_SERIAL_SHIFT: u32 = 26; // Above all netPlAction state and sheathe bits.
const MAX_SERIAL: u8 = 63;
const MAX_VALUE: u8 = 7;
// Ordinary position snapshots keep their existing action stamp. Explicit
// events use bit 31 and the remaining bits outside GU's 4..7 nibble.
const EVENT_PRESENT: u32 = 1 << 31;
const EVENT_MAX: u32 = (1 << 27) - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    style: u32,
    action: u8,
    serial: u8,
}

static SENT: Shared<Option<(u32, Stamp)>> = Shared::new(None);
static ACCEPTED: remote::PeerStates<Stamp> = remote::PeerStates::new();
static SERVICE: remote::PositionService = remote::PositionService(bind);
static EVENTS: remote::PositionEvents = remote::PositionEvents {
    bind: bind_event,
    send: send_event,
    peer: event_peer,
};

static EVENT_TO_SEND: Shared<Option<(u32, u32)>> = Shared::new(None);

fn event_receiver(entry: &Entry) -> Option<remote::ReceivePositionEvent> {
    let handler = entry.multiplayer.position_event.load(Ordering::Acquire);
    (!handler.is_null())
        .then(|| unsafe { core::mem::transmute::<*mut (), remote::ReceivePositionEvent>(handler) })
}

extern "C" fn bind_event(id: u32, receive: remote::ReceivePositionEvent) -> bool {
    let Some(entry) = registered_entry(id)
        .filter(|entry| entry.kind.is_some() && entry.multiplayer.playback().is_some())
    else {
        return false;
    };
    entry
        .multiplayer
        .position_event
        .store(receive as *mut (), Ordering::Release);
    true
}

extern "C" fn event_peer(style: u32, actor: u32) -> bool {
    on_game_thread()
        && unsafe { peer_entry(actor) }
            .is_some_and(|entry| entry.style.id == style && event_receiver(entry).is_some())
}

extern "C" fn send_event(style: u32, actor: u32, payload: u32) -> bool {
    if payload > EVENT_MAX
        || !on_game_thread()
        || unsafe { local_hunter() } != actor
        || unsafe { local_entry(actor) }
            .filter(|entry| entry.style.id == style)
            .and_then(event_receiver)
            .is_none()
    {
        return false;
    }
    unsafe {
        let network = read::<u32>(forge::mem::text_addr(), 0x18843f0);
        if network == 0 {
            return false;
        }
        // The same quest position process selected by GU 29C368.
        let process = gu!(0x227b3c, network, 6);
        if process == 0 || read::<u8>(process, 16) == 0 || read::<u32>(process, 36) == 0 {
            return false;
        }
        EVENT_TO_SEND.with(|event| {
            *event = Some((
                process,
                EVENT_PRESENT | (payload & 15) | ((payload & !15) << 4),
            ));
        });
        gu!(0x200438, process, actor);
        EVENT_TO_SEND.with(|event| *event = None);
    }
    true
}

fn codec(entry: &Entry) -> Option<&'static remote::ActionState> {
    unsafe { entry.multiplayer.position.load(Ordering::Acquire).as_ref() }
}

extern "C" fn bind(id: u32, state: &'static remote::ActionState) -> bool {
    let Some(entry) = registered_entry(id)
        .filter(|entry| entry.kind.is_some() && entry.multiplayer.playback().is_some())
    else {
        return false;
    };
    entry
        .multiplayer
        .position
        .store(ptr::from_ref(state).cast_mut(), Ordering::Release);
    true
}

pub(super) unsafe fn stamp_action(packet: u32, actor: u32, entry: &Entry) {
    let (group, slot) = unsafe { packet_action(packet) };
    let Some(id) = action::custom_id(group, slot)
        .filter(|&id| entry.style.actions.contains(id) && codec(entry).is_some())
    else {
        return;
    };
    let serial = SENT.with(|saved| {
        let serial = saved.map_or(1, |(_, last)| last.serial % MAX_SERIAL + 1);
        *saved = Some((
            actor,
            Stamp {
                style: entry.style.id,
                action: id,
                serial,
            },
        ));
        serial
    });
    unsafe {
        let flags = read::<u32>(packet, ACTION_FLAGS);
        set(
            packet,
            ACTION_FLAGS,
            flags | (u32::from(serial) << ACTION_SERIAL_SHIFT),
        );
    }
}

pub(super) unsafe fn accept_action(packet: u32, actor: u32) {
    let Some(entry) = (unsafe { peer_entry(actor) }).filter(|entry| codec(entry).is_some()) else {
        return;
    };
    let Some(index) =
        (unsafe { remote::Roster::current() }).and_then(|roster| roster.peer_index(actor))
    else {
        return;
    };
    let (group, slot) = unsafe { packet_action(packet) };
    if unsafe { action::current(actor) } != (group, slot) {
        return;
    }
    let serial = (unsafe { read::<u32>(packet, ACTION_FLAGS) } >> ACTION_SERIAL_SHIFT) as u8;
    let stamp = action::custom_id(group, slot)
        .filter(|&id| serial != 0 && entry.style.actions.contains(id))
        .map(|id| Stamp {
            style: entry.style.id,
            action: id,
            serial,
        });
    ACCEPTED.update(index, actor, |saved, _| *saved = stamp);
}

pub(super) fn observe(index: usize, actor: u32) {
    ACCEPTED.update(index, actor, |_, _| {});
}

unsafe fn is_position(packet: u32) -> bool {
    packet != 0 && unsafe { read::<u32>(packet, 0) } == forge::mem::text_addr() + VTABLE
}

unsafe fn position_extension() -> Option<u32> {
    let actor = unsafe { local_hunter() };
    let entry = unsafe { local_entry(actor) }?;
    let state = codec(entry)?;
    let (group, slot) = unsafe { action::current(actor) };
    let id = action::custom_id(group, slot)?;
    let (owner, stamp) = SENT.with(|saved| *saved)?;
    if (owner, stamp.style, stamp.action) != (actor, entry.style.id, id) {
        return None;
    }
    let value = unsafe { (state.encode)(actor) };
    (value <= MAX_VALUE).then_some(
        POSITION_PRESENT
            | (u32::from(value) << POSITION_STATE_SHIFT)
            | (u32::from(id) << POSITION_ACTION_SHIFT)
            | (u32::from(stamp.serial) << POSITION_SERIAL_SHIFT),
    )
}

#[forge::hook(offset = 0x9fc3d8)]
unsafe fn send_packet(process: u32, target: u32, packet: u32, priority: u32, channel: u32) -> u32 {
    if priority == 4
        && channel == 4160
        && unsafe { is_position(packet) }
        && on_game_thread()
        && let Some(extension) = EVENT_TO_SEND
            .with(|event| {
                event
                    .filter(|(owner, _)| *owner == process)
                    .map(|(_, flags)| flags)
            })
            .or_else(|| unsafe { position_extension() })
    {
        let flags = unsafe { read::<u32>(packet, POSITION_FLAGS) };
        unsafe { set(packet, POSITION_FLAGS, flags | extension) };
        let result = original!(process, target, packet, priority, channel);
        unsafe { set(packet, POSITION_FLAGS, flags) };
        result
    } else {
        original!(process, target, packet, priority, channel)
    }
}

unsafe fn apply_extension(packet: u32, actor: u32) -> Option<()> {
    if !unsafe { is_position(packet) } || !on_game_thread() {
        return None;
    }
    let flags = unsafe { read::<u32>(packet, POSITION_FLAGS) };
    if flags & EVENT_PRESENT != 0 {
        let receive = unsafe { peer_entry(actor) }.and_then(event_receiver)?;
        let position = unsafe { read::<[f32; 3]>(packet, 16) };
        if position.iter().all(|coordinate| coordinate.is_finite()) {
            receive(&remote::PositionEvent {
                actor,
                position,
                payload: ((flags & 0x7fff_ff00) >> 4) | (flags & 15),
            });
        }
        return Some(());
    }
    if flags & POSITION_PRESENT == 0 {
        return None;
    }
    let entry = unsafe { peer_entry(actor) }?;
    let state = codec(entry)?;
    let index = unsafe { remote::Roster::current() }?.peer_index(actor)?;
    let (group, slot) = unsafe { action::current(actor) };
    let id = action::custom_id(group, slot)?;
    if id != (flags >> POSITION_ACTION_SHIFT) as u8 {
        return None;
    }
    let serial = (flags >> POSITION_SERIAL_SHIFT) as u8 & MAX_SERIAL;
    let expected = Stamp {
        style: entry.style.id,
        action: id,
        serial,
    };
    let mut accepted = false;
    ACCEPTED.update(index, actor, |saved, _| accepted = *saved == Some(expected));
    if accepted {
        let value = (flags >> POSITION_STATE_SHIFT) as u8 & MAX_VALUE;
        unsafe { (state.decode)(actor, value) };
    }
    Some(())
}

#[forge::hook(offset = 0x1ffff4)]
unsafe fn unpack_position(packet: u32, actor: u32) -> u32 {
    let result = original!(packet, actor);
    unsafe { apply_extension(packet, actor) };
    result
}

pub(super) fn install(base: u32) {
    if unsafe { remote::publish_position(&SERVICE) } {
        unsafe { remote::publish_position_events(&EVENTS) };
        forge::install_hook!(base, send_packet);
        forge::install_hook!(base, unpack_position);
    }
}
