//! GU 3A75C8: the mixed hunt/capture/slay goal path, separate from native record indexing.
use super::{read, write};
use mhgu_sdk::monster::species::SpeciesId;

const ACTIVE_HUNT: u32 = 9;
const COMPLETE: u32 = 0x20;
const FAILED: u32 = 0x40;
const CAPTURE: u32 = 0x80;
const SLAY: u32 = 0x100;
const ALL: u32 = 0x1000;
const LINKED: u32 = 0x4000;
const FIRST_GOAL: usize = 236;

// GU tracks spawned monsters in five 16-byte records and configures five
// separate 36-byte spawn slots. An all-monsters goal must account for both.
unsafe fn advance_all(quest: u32, progress: usize, id: SpeciesId) -> bool {
    unsafe {
        let mut hunted = read::<u16>(quest, progress);
        let mut tracked = 0u16;
        let mut configured = 0u32;
        for slot in 0..5 {
            let at = 4824 + 16 * slot;
            let key = read::<u16>(quest, at);
            if key != 0 {
                tracked |= 1 << slot;
            }
            if key == id.packed() && read::<u8>(quest, at + 2) == 0 {
                hunted |= 1 << slot;
            }
            if read::<u16>(quest, 4900 + 36 * slot) != 0 {
                configured += 1;
            }
        }
        write(quest, progress, hunted);
        configured == tracked.count_ones() && hunted & tracked == tracked
    }
}

pub(super) unsafe fn advance(q: u32, id: SpeciesId, captured: bool) -> bool {
    unsafe {
        let mut completed = false;
        for i in 0..3 {
            let at = FIRST_GOAL + 12 * i;
            let flags = read::<u32>(q, at);
            if flags & FAILED != 0 {
                continue;
            }
            if flags & ACTIVE_HUNT == ACTIVE_HUNT {
                if read::<u16>(q, at + 4) != id.packed() {
                    continue;
                }
                if flags & COMPLETE == 0
                    && ((!captured && flags & CAPTURE != 0) || (captured && flags & SLAY != 0))
                {
                    write(q, at, flags | FAILED);
                    write(q, 94, 1u8);
                    continue;
                }
                if flags & LINKED != 0 && read::<u32>(q, FIRST_GOAL) & LINKED == 0 {
                    return false;
                }
                let left = read::<u16>(q, at + 6).saturating_sub(1);
                write(q, at + 6, left);
                if left != 0 {
                    continue;
                }
            } else if flags & ALL != 0 {
                if !advance_all(q, at + 6, id) {
                    continue;
                }
            } else {
                if i < 2
                    && flags & 0x60200 != 0
                    && flags & COMPLETE == 0
                    && read::<u16>(q, at + 4) == id.packed()
                {
                    write(q, at, flags | FAILED);
                    write(q, 94, 1u8);
                }
                continue;
            }
            completed = true;
            if flags & COMPLETE == 0 && !(3..10).contains(&read::<u8>(q, 80)) {
                write(q, at, flags | COMPLETE);
                if i == 1 {
                    let first = read::<u32>(q, FIRST_GOAL);
                    if flags & LINKED != 0 && first & (COMPLETE | FAILED) == 0 {
                        write(q, FIRST_GOAL, first | COMPLETE);
                    }
                } else if i == 2 {
                    write(q, 99, 1u8);
                }
            }
        }
        // These two helpers use the full species ID (not a native dense index).
        let base = forge::mem::text_addr();
        let supply: unsafe extern "C" fn(u32, u32, u32, u32) =
            core::mem::transmute(base + 0x3a400c);
        let notify: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(base + 0x3a83a4);
        supply(q, 1, id.packed().into(), 1);
        notify(q + 6740, id.packed().into(), 1);
        completed
    }
}
