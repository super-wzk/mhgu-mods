use crate::weapon::{gu, read, set};
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhgu_sdk::{
    combat::collision::hurt::{
        GroundHoldService, end_ground_hold_replica, initialize_ground_hold, publish_ground_hold,
    },
    service::on_game_thread,
    weapon::style::remote::{PLAYER_COUNT, Roster},
};

static ENTERING: AtomicU32 = AtomicU32::new(0);
static DURATION: AtomicU32 = AtomicU32::new(0);
const INITIALIZED: u32 = u32::MAX;

const ACTION_GROUP: u8 = 10;
const ACTION_SLOT: u8 = 31;
// netEmAction::mParam.status_bit is serialized as a u32. CB330 and CC154
// use bits 0–19. Encode the duration in whole GU frames in the upper 12 bits.
const HOLD_SHIFT: u32 = 20;
const MAX_HOLD_FRAMES: u32 = u32::MAX >> HOLD_SHIFT;

fn begin_entry(actor: u32, frames: f32) -> bool {
    if ENTERING
        .compare_exchange(0, actor, Relaxed, Relaxed)
        .is_err()
    {
        return false;
    }
    DURATION.store(frames.to_bits(), Relaxed);
    true
}

#[forge::hook(offset = 0xa2c04)]
unsafe fn paralysis_entry(actor: u32) -> u32 {
    if ENTERING.load(Relaxed) == actor
        && let Some(status) =
            unsafe { initialize_ground_hold(actor, f32::from_bits(DURATION.load(Relaxed))) }
    {
        ENTERING.store(INITIALIZED, Relaxed);
        return status;
    }
    original!(actor)
}

unsafe fn sync_status(actor: u32) -> Option<u32> {
    let status = unsafe { read::<u32>(actor, 5160) };
    (status != 0
        && unsafe { read::<u8>(status, 424) } & 8 != 0
        && unsafe { read::<u16>(status, 438) } != u16::MAX)
        .then_some(status)
}

unsafe fn sync_action(actor: u32, data: u32) -> bool {
    unsafe {
        sync_status(actor).is_some_and(|status| {
            read::<u8>(data, 3) == 1
                && read::<u8>(data, 1) == read::<u8>(actor, 29732)
                && read::<u16>(data, 4) == read::<u16>(status, 438)
                && read::<u8>(data, 6) == read::<u8>(status, 23744)
        })
    }
}

unsafe fn is_hold_action(data: u32) -> bool {
    unsafe { (read::<u8>(data, 107), read::<u8>(data, 69)) == (ACTION_GROUP, ACTION_SLOT) }
}

// CB330 builds this payload while 76154 still has the action entry in flight.
#[forge::hook(offset = 0x1eb1e8)]
unsafe fn pack_monster_action(packet: u32, data: u32) -> u32 {
    let result = original!(packet, data);
    let actor = ENTERING.load(Relaxed);
    if actor == 0 || actor == INITIALIZED || !on_game_thread() {
        return result;
    }
    unsafe {
        if read::<u8>(actor, 29728) != 0 && sync_action(actor, data) && is_hold_action(data) {
            let frames = f32::from_bits(DURATION.load(Relaxed)) as u32;
            if (1..=MAX_HOLD_FRAMES).contains(&frames) {
                set(packet, 64, read::<u32>(packet, 64) | (frames << HOLD_SHIFT));
            }
        }
    }
    result
}

#[forge::hook(offset = 0xcc154)]
unsafe fn receive_monster_action(actor: u32, data: u32, member: u32) -> u32 {
    if !on_game_thread() {
        return original!(actor, data, member);
    }
    unsafe {
        if read::<u8>(actor, 29728) != 0 || !sync_action(actor, data) {
            return original!(actor, data, member);
        }
        end_ground_hold_replica(actor);
        let frames = read::<u32>(data, 112) >> HOLD_SHIFT;
        if frames != 0 && is_hold_action(data) && begin_entry(actor, frames as f32) {
            let result = original!(actor, data, member);
            ENTERING.store(0, Relaxed);
            return result;
        }
    }
    original!(actor, data, member)
}

unsafe extern "C" fn start(actor: u32, mut duration: f32) -> bool {
    if !on_game_thread() {
        return false;
    }
    unsafe {
        let base = forge::mem::text_addr();
        let network = read::<u32>(base, 0x18843f0);
        let connected = network != 0 && read::<u8>(network, 1598) == 1;
        // 5BB10 classifies the scene, including solo gathering-hub hunts.
        // Only a live remote hunter requires a transport for an owned monster.
        let peers =
            Roster::current().is_some_and(|r| (0..PLAYER_COUNT).any(|i| r.peer(i).is_some()));
        let room_scene: unsafe extern "C" fn() -> u32 = core::mem::transmute(base + 0x5bb10);
        if connected || (peers && room_scene() != 0) {
            if !connected || sync_status(actor).is_none() || duration > MAX_HOLD_FRAMES as f32 {
                return false;
            }
            let process = gu!(0x227b3c, network, 60);
            if process == 0 || read::<u8>(process, 16) == 0 || read::<u32>(process, 36) == 0 {
                return false;
            }
            duration = (duration as u32).max(1) as f32;
        }
    }
    if !begin_entry(actor, duration) {
        return false;
    }
    // 76AFC selects and enters the native action synchronously.
    let request: unsafe extern "C" fn(u32, u32, u32) -> u32 =
        unsafe { core::mem::transmute(forge::mem::text_addr() + 0x76afc) };
    unsafe { request(actor, u32::from(ACTION_GROUP), u32::from(ACTION_SLOT)) };
    ENTERING.swap(0, Relaxed) == INITIALIZED
}

pub(super) fn install(base: u32) {
    static SERVICE: GroundHoldService = GroundHoldService(start);
    if unsafe { publish_ground_hold(&SERVICE) } {
        forge::install_hook!(base, paralysis_entry);
        forge::install_hook!(base, pack_monster_action);
        forge::install_hook!(base, receive_monster_action);
    }
}
