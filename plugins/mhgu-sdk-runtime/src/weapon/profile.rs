//! Style identity carried alongside GU's room equipment records.
use super::{
    gu, inventory, local_hunter,
    style::{Entry, active_equipment, registered_entry},
};
use mhgu_sdk::sync::Shared;

const MANAGER: u32 = 0x1885b58;
const NETWORK: u32 = 0x18843f0;
const ATTR: u32 = 1400;
const STOCK_FLAGS: u32 = 0xf;
// netPlData serializes all 32 bits of mAttr. GU checks bit 0 to request a reply.
const TAG: u32 = 0x4d70_0000;
const TAG_MASK: u32 = 0xfff0_0000;
const EQUIPMENT_TAG: u32 = 0x9800_0000;
const EQUIPMENT_MASK: u32 = 0xfe00_0000;
const PLAYERS: usize = 4;

#[derive(Clone, Copy)]
struct PeerStyle {
    manager: u32,
    id: u32,
    weapon: u32,
    upgrade: Option<u8>,
}

#[derive(Clone, Copy)]
pub(super) struct DisplayEquipment {
    pub definition: inventory::RegisteredEquipment,
    pub upgrade: u16,
}

impl DisplayEquipment {
    pub fn registered(id: u16, upgrade: u16) -> Option<Self> {
        let definition = inventory::definition(id)?.1;
        definition.levels.get(usize::from(upgrade))?;
        Some(Self {
            definition,
            upgrade,
        })
    }
}

static PEERS: Shared<[Option<PeerStyle>; PLAYERS]> = Shared::new([None; PLAYERS]);
static RECEIVING: Shared<Option<(u32, u32, PeerStyle)>> = Shared::new(None);

unsafe fn read<T: Copy>(address: u32) -> T {
    unsafe { (address as *const T).read_unaligned() }
}

pub(super) unsafe fn equipment_weapon(equipment: u32) -> u32 {
    u32::from(unsafe { read::<u8>(equipment + 1236) })
}

pub(super) fn selection_changed() {
    unsafe {
        let base = forge::mem::text_addr();
        let network: u32 = read(base + NETWORK);
        if network != 0 {
            let dirty: unsafe extern "C" fn(u32) = core::mem::transmute(base + 0x22e474);
            dirty(network);
        }
    }
}

pub(super) unsafe fn status_entry(equipment: u32) -> Option<&'static Entry> {
    unsafe { status(equipment).map(|v| v.0) }
}
pub(super) unsafe fn display(equipment: u32) -> Option<DisplayEquipment> {
    unsafe { status(equipment).and_then(|v| v.1) }
}

unsafe fn status(equipment: u32) -> Option<(&'static Entry, Option<DisplayEquipment>)> {
    unsafe {
        let base = forge::mem::text_addr();
        let manager: u32 = read(base + MANAGER);
        if manager == 0 || equipment == 0 {
            return None;
        }
        let current: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(base + 0x275a30);
        let actor = local_hunter();
        if equipment == current(manager) || (actor != 0 && equipment == read::<u32>(actor + 5060)) {
            return inventory::selected()
                .and_then(|(item, entry)| {
                    Some((
                        entry,
                        Some(DisplayEquipment::registered(
                            item.equipment.get(),
                            item.upgrade,
                        )?),
                    ))
                })
                .or_else(|| active_equipment(equipment).map(|entry| (entry, None)));
        }

        let peer_equipment: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(base + 0x27617c);
        let peers = PEERS.with(|peers| *peers);
        for (slot, peer) in peers.into_iter().enumerate() {
            if let Some(peer) = peer
                && peer.manager == manager
                && peer_equipment(manager, slot as u32) == equipment
                && peer.weapon == equipment_weapon(equipment)
            {
                return resolved(peer).map(|entry| (entry, peer_display(peer)));
            }
        }
        None
    }
}

fn peer_display(peer: PeerStyle) -> Option<DisplayEquipment> {
    DisplayEquipment::registered(peer.id as u16, u16::from(peer.upgrade?))
}

fn resolved(peer: PeerStyle) -> Option<&'static Entry> {
    if peer.upgrade.is_some() {
        peer_display(peer)?;
        return inventory::definition(peer.id as u16).map(|v| v.0);
    }
    registered_entry(peer.id)
        .filter(|entry| entry.kind.is_none() && entry.style.weapon == peer.weapon)
}

// A tagged but unresolved style must suppress the fallback for untagged packets.
unsafe fn peer_in_slot(slot: usize, equipment: u32) -> Option<PeerStyle> {
    if equipment == 0 {
        return None;
    }
    let manager: u32 = unsafe { read(forge::mem::text_addr() + MANAGER) };
    let peer = PEERS.with(|peers| peers.get(slot).copied().flatten())?;
    if manager != peer.manager || unsafe { equipment_weapon(equipment) } != peer.weapon {
        return None;
    }
    Some(peer)
}

pub(super) unsafe fn peer_style(slot: usize, equipment: u32) -> Option<Option<&'static Entry>> {
    unsafe { peer_in_slot(slot, equipment).map(resolved) }
}

pub(super) unsafe fn peer_model(actor: u32, equipment: u32) -> Option<DisplayEquipment> {
    unsafe {
        let manager: u32 = read(forge::mem::text_addr() + MANAGER);
        if manager == 0 || actor == 0 || actor == local_hunter() {
            return None;
        }
        // The four native room/quest actors occupy manager +28 through +40.
        let slot = (0..PLAYERS).find(|&i| read::<u32>(manager + 28 + 4 * i as u32) == actor)?;
        peer_display(peer_in_slot(slot, equipment)?)
    }
}

unsafe fn advertised(equipment: u32, flags: u32) -> u32 {
    if equipment == 0 || flags & !STOCK_FLAGS != 0 {
        return flags;
    }
    let selected = unsafe { inventory::is_native_local(equipment) }
        .then(inventory::selected)
        .flatten();
    if let Some((item, _)) = selected {
        // Current GU senders pass only the reply bit. Leave any other flags intact.
        if flags & !1 == 0 {
            return EQUIPMENT_TAG
                | (u32::from(item.upgrade) << 17)
                | (u32::from(item.equipment.get()) << 1)
                | flags;
        }
        return flags;
    }
    let id = unsafe { active_equipment(equipment) }.map_or(0, |entry| entry.style.id);
    if id == 0 {
        flags
    } else {
        TAG | (id << 4) | flags
    }
}

#[forge::hook(offset = 0x202640)]
unsafe fn broadcast(process: u32, equipment: u32, flags: u32) -> u32 {
    let flags = unsafe { advertised(equipment, flags) };
    original!(process, equipment, flags)
}

#[forge::hook(offset = 0x202764)]
unsafe fn unicast(process: u32, equipment: u32, flags: u32, target: u32) -> u32 {
    let flags = unsafe { advertised(equipment, flags) };
    original!(process, equipment, flags, target)
}

#[forge::hook(offset = 0x202894)]
unsafe fn receive(process: u32, packet: u32, sender: u32) {
    if sender as usize >= PLAYERS {
        return original!(process, packet, sender);
    }
    unsafe {
        let prior = PEERS
            .with(|peers| peers[sender as usize])
            .and_then(peer_display);
        let flags: u32 = read(packet + ATTR);
        let manager: u32 = read(forge::mem::text_addr() + MANAGER);
        let extension = flags & EQUIPMENT_MASK == EQUIPMENT_TAG;
        let style = (extension || flags & TAG_MASK == TAG).then(|| PeerStyle {
            upgrade: extension.then_some((flags >> 17) as u8),
            manager,
            id: (flags >> if extension { 1 } else { 4 }) & u16::MAX as u32,
            weapon: equipment_weapon(packet + 8),
        });
        let previous = RECEIVING.with(|pending| {
            core::mem::replace(pending, style.map(|style| (sender, packet + 8, style)))
        });
        original!(process, packet, sender);
        RECEIVING.with(|pending| *pending = previous);
        let shown = |display: DisplayEquipment| (display.definition.equipment.id, display.upgrade);
        if manager != 0 && prior.map(shown) != style.and_then(peer_display).map(shown) {
            let actor: u32 = read(manager + 28 + sender * 4);
            let equipment = inventory::actor_equipment(actor);
            if equipment != 0
                && actor != local_hunter()
                && (7..=21).contains(&(read::<u16>(equipment + 28) & 31))
            {
                gu!(0x2861c4, actor, equipment + 24);
            }
        }
    }
}

fn retire(slot: u32) {
    PEERS.with(|peers| {
        if let Some(peer) = peers.get_mut(slot as usize) {
            *peer = None;
        }
    });
}

// Both the hunter and Prowler packets pass through the native record writer.
#[forge::hook(offset = 0x274400)]
unsafe fn replace_equipment(manager: u32, slot: u32, equipment: u32) -> u32 {
    retire(slot);
    let out = original!(manager, slot, equipment);
    let received = RECEIVING.with(|pending| {
        pending.filter(|&(sender, address, style)| {
            sender == slot && address == equipment && style.manager == manager
        })
    });
    if let Some((_, _, style)) = received {
        PEERS.with(|peers| peers[slot as usize] = Some(style));
    }
    out
}

#[forge::hook(offset = 0x2776d4)]
unsafe fn remove_player(manager: u32, slot: u32) -> u32 {
    retire(slot);
    original!(manager, slot)
}

#[forge::hook(offset = 0x273be0)]
unsafe fn reset_manager(manager: u32) -> u32 {
    PEERS.with(|peers| *peers = [None; PLAYERS]);
    original!(manager)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, broadcast);
    forge::install_hook!(base, unicast);
    forge::install_hook!(base, receive);
    forge::install_hook!(base, replace_equipment);
    forge::install_hook!(base, remove_player);
    forge::install_hook!(base, reset_manager);
}
