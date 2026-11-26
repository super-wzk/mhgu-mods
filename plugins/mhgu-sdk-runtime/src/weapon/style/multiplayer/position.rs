//! Optional expanded-weapon state on GU's native periodic position packet.
use super::{Entry, local_entry, local_hunter, packet_action, peer_entry, registered_entry};
use crate::weapon::{read, set};
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    style: u32,
    action: u8,
    serial: u8,
}

static SENT: Shared<Option<(u32, Stamp)>> = Shared::new(None);
static ACCEPTED: remote::PeerStates<Stamp> = remote::PeerStates::new();
static SERVICE: remote::PositionService = remote::PositionService(bind);

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
        && let Some(extension) = unsafe { position_extension() }
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
        forge::install_hook!(base, send_packet);
        forge::install_hook!(base, unpack_position);
    }
}
