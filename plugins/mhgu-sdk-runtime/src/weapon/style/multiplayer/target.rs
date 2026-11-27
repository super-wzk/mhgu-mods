//! Monster contacts carried by GU's netEmUniqueData process.
use super::{Entry, local_entry, local_hunter, peer_entry, registered_entry};
use crate::weapon::{gu, read};
use core::sync::atomic::Ordering;
use mhgu_sdk::{
    combat::collision::hurt::{Contact, HurtScene, Target},
    service::on_game_thread,
    weapon::style::{hunter, remote},
};

const VTABLE: u32 = 0x1741604;
const CONTACT_TYPE: u8 = 9; // GU CB120 leaves this type unused.
const CONTACT_MAGIC: u32 = u32::from_le_bytes(*b"MHT1");

#[repr(C)]
struct MonsterData {
    priority: u8,
    master: u8,
    uid: u8,
    kind: u8,
    set_id: u16,
    revision: u8,
    enemy: u8,
    magic: u32,
    style: u32,
    offset: [f32; 3],
    part: u16,
    row: u8,
    serial: u8,
    area: u8,
    reserved: [u8; 7],
}

const _: () = assert!(core::mem::size_of::<MonsterData>() == 40);

static EVENTS: remote::TargetEvents = remote::TargetEvents { bind, send };

fn receiver(entry: &Entry) -> Option<remote::ReceiveTargetEvent> {
    let callback = entry.multiplayer.target_event.load(Ordering::Acquire);
    (!callback.is_null())
        .then(|| unsafe { core::mem::transmute::<*mut (), remote::ReceiveTargetEvent>(callback) })
}

extern "C" fn bind(style: u32, receive: remote::ReceiveTargetEvent) -> bool {
    let Some(entry) = registered_entry(style)
        .filter(|entry| entry.kind.is_some() && entry.multiplayer.playback().is_some())
    else {
        return false;
    };
    entry
        .multiplayer
        .target_event
        .store(receive as *mut (), Ordering::Release);
    true
}

extern "C" fn send(style: u32, actor: u32, serial: u8, target: &Target) -> bool {
    if !on_game_thread()
        || unsafe { local_hunter() } != actor
        || unsafe { local_entry(actor) }
            .filter(|entry| entry.style.id == style)
            .and_then(receiver)
            .is_none()
    {
        return false;
    }
    unsafe {
        let base = forge::mem::text_addr();
        let network = read::<u32>(base, 0x18843f0);
        if network == 0 {
            return false;
        }
        let process = gu!(0x227b3c, network, 68);
        if process == 0 || read::<u8>(process, 16) == 0 || read::<u32>(process, 36) == 0 {
            return false;
        }
        let contact = target.contact();
        let local_member: unsafe extern "C" fn() -> u32 = core::mem::transmute(base + 0x49694);
        let packet = MonsterData {
            priority: 0,
            master: local_member() as u8,
            uid: 0,
            kind: CONTACT_TYPE,
            set_id: contact.set_id,
            revision: contact.revision,
            enemy: target.local_slot(),
            magic: CONTACT_MAGIC,
            style,
            part: contact.part,
            row: contact.row,
            offset: contact.offset,
            serial,
            area: hunter::area(actor),
            reserved: [0; 7],
        };
        gu!(0x1f00f8, process, (&raw const packet) as u32);
    }
    true
}

unsafe fn receive(packet: u32, source: u32) -> Option<()> {
    let base = forge::mem::text_addr();
    if unsafe { read::<u32>(packet, 0) } != base + VTABLE {
        return None;
    }
    let data = unsafe { &*((packet + 8) as *const MonsterData) };
    if data.kind != CONTACT_TYPE || data.magic != CONTACT_MAGIC {
        return None;
    }
    let manager = unsafe { read::<u32>(base, 0x1885b58) };
    if manager == 0 {
        return None;
    }
    // Use the source slot supplied by GU's procedure, just as netPlPositionQuest does.
    let actor = unsafe { gu!(0x27724c, manager, source) };
    let entry = unsafe { peer_entry(actor) }.filter(|entry| entry.style.id == data.style)?;
    let receive = receiver(entry)?;
    let scene = unsafe { HurtScene::from_raw(local_hunter()) }?;
    if scene.area() != data.area || unsafe { hunter::area(actor) } != data.area {
        return None;
    }
    let target = scene.resolve_contact(Contact {
        set_id: data.set_id,
        revision: data.revision,
        part: data.part,
        row: data.row,
        offset: data.offset,
    })?;
    receive(&remote::TargetEvent {
        actor,
        serial: data.serial,
        target,
    });
    Some(())
}

#[forge::hook(offset = 0x1f51f4)]
unsafe fn receive_unique_data(process: u32, packet: u32, incoming: u32, source: u32) -> u32 {
    let result = original!(process, packet, incoming, source);
    if result == 1 && incoming == 1 && source < remote::PLAYER_COUNT as u32 && on_game_thread() {
        unsafe { receive(packet, source) };
    }
    result
}

pub(super) fn install(base: u32) {
    if unsafe { remote::publish_target_events(&EVENTS) } {
        forge::install_hook!(base, receive_unique_data);
    }
}
