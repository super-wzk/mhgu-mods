//! Native positional requests. The queue retains its own reference, even
//! if a hunter changes weapons before the audio thread consumes the entry.
use core::{
    ffi::CStr,
    sync::atomic::{AtomicU32, Ordering},
};
static REQUEST: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
const PATH: [&CStr; 3] = [
    c"sound\\se\\pl\\frontier_gs\\blade",
    c"sound\\se\\pl\\frontier_gs\\common",
    c"sound\\se\\pl\\frontier_gs\\iron",
];
pub(super) unsafe fn play(p: u32, common: bool, id: u8) {
    unsafe { request(p, usize::from(common), id) }
}
unsafe fn request(player: u32, index: usize, id: u8) {
    unsafe {
        let base = forge::mem::text_addr();
        let engine = *((base + 0x1896f5c) as *const u32);
        if engine == 0 {
            return;
        }
        let cache = &REQUEST[index];
        let mut res = cache.load(Ordering::Acquire);
        if res == 0 {
            let mgr = *((base + 0x2126a64) as *const u32);
            if mgr == 0 {
                return;
            }
            let vtable = *(mgr as *const u32);
            let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(*((vtable + 48) as *const u32));
            let release: unsafe extern "C" fn(u32) = core::mem::transmute(base + 0x887660);
            res = load(mgr, base + 0x2125bec, PATH[index].as_ptr(), 1);
            if res == 0 {
                return;
            }
            if *((res + 80) as *const u32) & 1 == 0 {
                release(res);
                return;
            }
            if let Err(other) = cache.compare_exchange(0, res, Ordering::AcqRel, Ordering::Acquire)
            {
                release(res);
                res = other;
            }
        }
        // 4EF7A4 submits positioned hunter sounds as type 7 with the actor
        // as owner. 4EEAA4 accepts our private request bank for the same queue.
        let queue: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) -> u32 =
            core::mem::transmute(base + 0x4eeaa4);
        let cue = 4096 + u32::from(id);
        let lookup: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(base + 0xb68280);
        // A request can be enabled in the sequence but unvoiced by this instrument.
        if lookup(res, cue) != 0 {
            queue(engine, 7, res, cue, player, player + 64);
        }
    }
}

const TRACKS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/assets/frontier_gs_sound.fgse"
));
#[derive(Clone, Copy, Default)]
pub(super) struct Clock {
    clip: u16,
    frame: f32,
}
impl Clock {
    pub(super) unsafe fn update(
        &mut self,
        p: u32,
        clip: Option<u16>,
        frame: f32,
        first: Option<f32>,
    ) {
        let Some(clip @ 1000..1500) = clip else {
            self.clip = 0;
            return;
        };
        let from = if let Some(start) = first {
            start - 1.
        } else if self.clip != clip || frame < self.frame {
            frame - 1.
        } else {
            self.frame
        };
        self.clip = clip;
        self.frame = frame;
        let index = 4 * usize::from(clip - 1000);
        let half = |p: usize| u16::from_le_bytes([TRACKS[p], TRACKS[p + 1]]) as usize;
        let (at, n) = (half(index), half(index + 2));
        for row in TRACKS[at..at + 12 * n].chunks_exact(12) {
            let time = f32::from(u16::from_le_bytes([row[0], row[1]]));
            if time <= from || time > frame {
                continue;
            }
            let choices = &row[4..4 + 2 * usize::from(row[3])];
            let sum = choices
                .chunks_exact(2)
                .map(|c| u32::from(c[1]))
                .sum::<u32>();
            if sum == 0 {
                continue;
            }
            let mut roll = 0;
            if choices.len() > 2 {
                let rand: unsafe extern "C" fn() -> u32 =
                    unsafe { core::mem::transmute(forge::mem::text_addr() + 0x27c8f8) };
                roll = unsafe { rand() } % sum;
            }
            for c in choices.chunks_exact(2) {
                if roll < u32::from(c[1]) {
                    if c[0] != 255 {
                        unsafe {
                            request(p, usize::from(row[2]), c[0]);
                        }
                    }
                    break;
                }
                roll -= u32::from(c[1]);
            }
        }
    }
}
pub(super) unsafe fn charge(p: u32, cue: u8) {
    unsafe { request(p, 2, cue) }
}
