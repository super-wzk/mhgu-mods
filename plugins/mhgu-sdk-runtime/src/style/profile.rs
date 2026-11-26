//! Style identity carried alongside GU's room equipment records.
use super::{Entry, active, local_hunter, registered_entry, selected};
use mhgu_sdk::sync::Shared;

const MANAGER: u32 = 0x1885b58;
const NETWORK: u32 = 0x18843f0;
const ATTR: u32 = 1400;
const STOCK_FLAGS: u32 = 0xf;
// netPlData serializes all 32 bits of mAttr. GU checks bit 0 to request a reply.
const TAG: u32 = 0x4d70_0000;
const TAG_MASK: u32 = 0xfff0_0000;
const PLAYERS: usize = 4;

#[derive(Clone, Copy)]
struct PeerStyle {
    manager: u32,
    id: u32,
    weapon: u32,
}

static PEERS: Shared<[Option<PeerStyle>; PLAYERS]> = Shared::new([None; PLAYERS]);

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
    unsafe {
        let base = forge::mem::text_addr();
        let manager: u32 = read(base + MANAGER);
        if manager == 0 || equipment == 0 {
            return None;
        }
        let current: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(base + 0x275a30);
        let weapon = equipment_weapon(equipment);
        if equipment == current(manager) {
            return active(weapon);
        }
        let actor = local_hunter();
        if actor != 0 && equipment == read(actor + 5060) {
            return active(weapon);
        }

        let peer_equipment: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(base + 0x27617c);
        let peers = PEERS.with(|peers| *peers);
        for (slot, peer) in peers.into_iter().enumerate() {
            if let Some(peer) = peer
                && peer.manager == manager
                && peer.weapon == weapon
                && peer_equipment(manager, slot as u32) == equipment
            {
                return registered_entry(peer.id).filter(|entry| entry.style.weapon == weapon);
            }
        }
        None
    }
}

unsafe fn advertised(equipment: u32, flags: u32) -> u32 {
    if equipment == 0 || flags & !STOCK_FLAGS != 0 {
        return flags;
    }
    let id = selected(unsafe { equipment_weapon(equipment) });
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
    original!(process, packet, sender);
    if (sender as usize) < PLAYERS {
        let flags: u32 = unsafe { read(packet + ATTR) };
        if flags & TAG_MASK == TAG {
            let style = PeerStyle {
                manager: unsafe { read(forge::mem::text_addr() + MANAGER) },
                id: (flags >> 4) & u16::MAX as u32,
                weapon: unsafe { equipment_weapon(packet + 8) },
            };
            PEERS.with(|peers| peers[sender as usize] = Some(style));
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
    original!(manager, slot, equipment)
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
