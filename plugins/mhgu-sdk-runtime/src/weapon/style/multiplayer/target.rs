//! Monster contacts and controller snapshots carried by GU's netEmUniqueData.
use super::{Entry, local_entry, local_hunter, peer_entry, registered_entry};
use crate::weapon::{gu, read};
use core::sync::atomic::Ordering;
use mhgu_sdk::{
    combat::collision::hurt::{Contact, HurtScene, Monster, MonsterSpawn, Target},
    service::on_game_thread,
    weapon::style::{
        hunter,
        remote::{self, MonsterCommand, MonsterExchange},
    },
};

const VTABLE: u32 = 0x1741604;
const EXTENSION_TYPE: u8 = 9; // GU CB120 leaves this type unused.
const CONTACT_MAGIC: u32 = u32::from_le_bytes(*b"MHT1");
const STATE_MAGIC: u32 = u32::from_le_bytes(*b"MHS1");
const EXCHANGE_MAGIC: u32 = u32::from_le_bytes(*b"MHE1");

#[repr(C)]
struct Header {
    priority: u8,
    master: u8,
    uid: u8,
    kind: u8,
    set_id: u16,
    revision: u8,
    enemy: u8,
}

impl Header {
    fn new(member: u8, set_id: u16, revision: u8, slot: u8) -> Self {
        Self {
            priority: 0,
            master: member,
            uid: 0,
            kind: EXTENSION_TYPE,
            set_id,
            revision,
            enemy: slot,
        }
    }
}

#[repr(C)]
struct MonsterData {
    header: Header,
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

#[repr(C)]
struct StateData {
    header: Header,
    magic: u32,
    style: u32,
    value: u32,
    serial: u16,
    species: u16,
    area: u8,
    command: u8,
    subject: u8,
    reserved: u8,
    words: [u32; 3],
}

const _: () = assert!(core::mem::size_of::<StateData>() == 40);

impl StateData {
    fn new(style: u32, member: u8, monster: &Monster<'_>, area: u8, serial: u16) -> Self {
        let spawn = monster.spawn();
        Self {
            header: Header::new(member, spawn.set_id, spawn.revision, monster.local_slot()),
            magic: STATE_MAGIC,
            style,
            value: 0,
            serial,
            species: spawn.kind,
            area,
            command: 0,
            subject: 0,
            reserved: 0,
            words: [0; 3],
        }
    }
}

static EVENTS: remote::TargetEvents = remote::TargetEvents { bind, send };
static STATES: remote::MonsterStateEvents = remote::MonsterStateEvents {
    bind: bind_state,
    send: send_state,
    bind_exchange,
    send_exchange,
};

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

fn state_receiver(entry: &Entry) -> Option<remote::ReceiveMonsterState> {
    let callback = entry.multiplayer.monster_state.load(Ordering::Acquire);
    (!callback.is_null())
        .then(|| unsafe { core::mem::transmute::<*mut (), remote::ReceiveMonsterState>(callback) })
}

extern "C" fn bind_state(style: u32, receive: remote::ReceiveMonsterState) -> bool {
    let Some(entry) = registered_entry(style).filter(|entry| entry.kind.is_some()) else {
        return false;
    };
    entry
        .multiplayer
        .monster_state
        .store(receive as *mut (), Ordering::Release);
    true
}

fn exchange_receiver(entry: &Entry) -> Option<remote::ReceiveMonsterExchange> {
    let callback = entry.multiplayer.monster_exchange.load(Ordering::Acquire);
    (!callback.is_null()).then(|| unsafe {
        core::mem::transmute::<*mut (), remote::ReceiveMonsterExchange>(callback)
    })
}

extern "C" fn bind_exchange(style: u32, receive: remote::ReceiveMonsterExchange) -> bool {
    let Some(entry) = registered_entry(style).filter(|entry| entry.kind.is_some()) else {
        return false;
    };
    entry
        .multiplayer
        .monster_exchange
        .store(receive as *mut (), Ordering::Release);
    true
}

unsafe fn local_member() -> u8 {
    let get: unsafe extern "C" fn() -> u32 =
        unsafe { core::mem::transmute(forge::mem::text_addr() + 0x49694) };
    unsafe { get() as u8 }
}

unsafe fn send_context() -> Option<(u32, u8)> {
    unsafe {
        let base = forge::mem::text_addr();
        let network = read::<u32>(base, 0x18843f0);
        if network == 0 {
            return None;
        }
        let process = gu!(0x227b3c, network, 68);
        if process == 0 || read::<u8>(process, 16) == 0 || read::<u32>(process, 36) == 0 {
            return None;
        }
        Some((process, local_member()))
    }
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
        let Some((process, member)) = send_context() else {
            return false;
        };
        let contact = target.contact();
        let packet = MonsterData {
            header: Header::new(
                member,
                contact.set_id,
                contact.revision,
                target.local_slot(),
            ),
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

extern "C" fn send_state(
    style: u32,
    spawn: &MonsterSpawn,
    serial: u16,
    value: u32,
    detail: &[u32; 2],
) -> bool {
    if !on_game_thread() || registered_entry(style).and_then(state_receiver).is_none() {
        return false;
    }
    unsafe {
        let Some(roster) = remote::Roster::current() else {
            return false;
        };
        let Some((process, member)) = send_context() else {
            return false;
        };
        let Some(scene) = HurtScene::from_raw(roster.local()) else {
            return false;
        };
        let Some(monster) = scene.find_spawn(*spawn).filter(|monster| {
            monster.locally_owned() && monster.network_controller() == Some(member)
        }) else {
            return false;
        };
        let mut data = StateData::new(style, member, &monster, scene.area(), serial);
        data.value = value;
        data.words[..2].copy_from_slice(detail);
        gu!(0x1f00f8, process, (&raw const data) as u32);
    }
    true
}

unsafe fn transmit_exchange(style: u32, exchange: &MonsterExchange) -> Option<()> {
    let roster = unsafe { remote::Roster::current() }?;
    let (process, member) = unsafe { send_context() }?;
    let peer = roster.peer(usize::from(exchange.member))?;
    let scene = unsafe { HurtScene::from_raw(roster.local()) }?;
    let monster = scene.find_spawn(exchange.spawn)?;
    if exchange.area != scene.area()
        || !unsafe { hunter::is_quest(peer) && hunter::area(peer) == scene.area() }
    {
        return None;
    }
    if exchange.command == MonsterCommand::Confirm {
        if !monster.locally_owned() || monster.network_controller() != Some(member) {
            return None;
        }
        unsafe { peer_entry(peer) }.filter(|entry| entry.style.id == style)?;
    } else {
        if monster.locally_owned() || monster.network_controller() != Some(exchange.member) {
            return None;
        }
        unsafe { local_entry(roster.local()) }.filter(|entry| entry.style.id == style)?;
    }
    let mut data = StateData::new(style, member, &monster, scene.area(), exchange.serial);
    data.magic = EXCHANGE_MAGIC;
    data.command = exchange.command as u8;
    data.subject = exchange.member;
    data.words = exchange.center.map(f32::to_bits);
    unsafe { gu!(0x1f00f8, process, (&raw const data) as u32) };
    Some(())
}

extern "C" fn send_exchange(style: u32, exchange: &MonsterExchange) -> bool {
    on_game_thread()
        && registered_entry(style)
            .and_then(exchange_receiver)
            .is_some()
        && unsafe { transmit_exchange(style, exchange) }.is_some()
}

unsafe fn receive_state(packet: u32, source: u32) -> Option<()> {
    let data = unsafe { &*((packet + 8) as *const StateData) };
    if u32::from(data.header.master) != source {
        return None;
    }
    let roster = unsafe { remote::Roster::current() }?;
    let peer = roster.peer(source as usize)?;
    let scene = unsafe { HurtScene::from_raw(roster.local()) }?;
    let spawn = MonsterSpawn {
        kind: data.species,
        set_id: data.header.set_id,
        revision: data.header.revision,
    };
    let monster = scene.find_spawn(spawn)?;
    if scene.area() != data.area
        || !unsafe { hunter::is_quest(peer) && hunter::area(peer) == data.area }
    {
        return None;
    }
    let entry = registered_entry(data.style)?;
    let from_controller =
        !monster.locally_owned() && monster.network_controller() == Some(data.header.master);
    let center = data.words.map(f32::from_bits);
    let command = match (data.magic, data.command) {
        (STATE_MAGIC, 0) if from_controller => {
            state_receiver(entry)?(&remote::MonsterState {
                actor: peer,
                spawn,
                controller: data.header.master,
                serial: data.serial,
                value: data.value,
                detail: [data.words[0], data.words[1]],
            });
            return Some(());
        }
        (EXCHANGE_MAGIC, 1)
            if monster.locally_owned() && center.iter().all(|axis| axis.is_finite()) =>
        {
            MonsterCommand::Request
        }
        (EXCHANGE_MAGIC, 2) if from_controller => MonsterCommand::Confirm,
        (EXCHANGE_MAGIC, 3) if monster.locally_owned() => MonsterCommand::End,
        _ => return None,
    };
    let member = unsafe { local_member() };
    if data.subject != member {
        return None;
    }
    if command == MonsterCommand::Confirm {
        unsafe { local_entry(roster.local()) }.filter(|entry| entry.style.id == data.style)?;
    } else {
        if monster.network_controller() != Some(member) {
            return None;
        }
        unsafe { peer_entry(peer) }.filter(|entry| entry.style.id == data.style)?;
    }
    exchange_receiver(entry)?(
        peer,
        &MonsterExchange {
            spawn,
            serial: data.serial,
            member: data.header.master,
            command,
            area: data.area,
            center,
        },
    );
    Some(())
}

unsafe fn receive(packet: u32, source: u32) -> Option<()> {
    let base = forge::mem::text_addr();
    if unsafe { read::<u32>(packet, 0) } != base + VTABLE {
        return None;
    }
    let data = unsafe { &*((packet + 8) as *const MonsterData) };
    if data.header.kind != EXTENSION_TYPE {
        return None;
    }
    match data.magic {
        STATE_MAGIC | EXCHANGE_MAGIC => return unsafe { receive_state(packet, source) },
        CONTACT_MAGIC => {}
        _ => return None,
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
        set_id: data.header.set_id,
        revision: data.header.revision,
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
        unsafe { remote::publish_monster_state(&STATES) };
        forge::install_hook!(base, receive_unique_data);
    }
}
