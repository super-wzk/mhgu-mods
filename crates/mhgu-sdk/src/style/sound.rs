//! Positional hunter sound requests on GU's audio queue.
use core::{
    ffi::CStr,
    sync::atomic::{AtomicU32, Ordering},
};

pub struct Bank {
    path: &'static CStr,
    resource: AtomicU32,
}

impl Bank {
    pub const fn new(path: &'static CStr) -> Self {
        Self {
            path,
            resource: AtomicU32::new(0),
        }
    }

    unsafe fn load(&self) -> Option<u32> {
        unsafe {
            let cached = self.resource.load(Ordering::Acquire);
            if cached != 0 {
                return Some(cached);
            }
            let manager = (crate::memory::entry(0x2126a64) as *const u32).read();
            if manager == 0 {
                return None;
            }
            let vtable = (manager as *const *const u32).read();
            let load: unsafe extern "C" fn(u32, usize, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(vtable.add(12).read());
            let resource = load(
                manager,
                crate::memory::entry(0x2125bec),
                self.path.as_ptr(),
                1,
            );
            if resource == 0 {
                return None;
            }
            let release: unsafe extern "C" fn(u32) =
                core::mem::transmute(crate::memory::entry(0x887660));
            if ((resource + 80) as *const u32).read() & 1 == 0 {
                release(resource);
                return None;
            }
            Some(
                match self.resource.compare_exchange(
                    0,
                    resource,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => resource,
                    Err(winner) => {
                        release(resource);
                        winner
                    }
                },
            )
        }
    }

    /// Queue a request using the hunter as the positional audio owner.
    ///
    /// # Safety
    /// The actor must be a live hunter on GU's game thread.
    pub unsafe fn play(&self, actor: u32, id: u8) {
        unsafe {
            let engine = (crate::memory::entry(0x1896f5c) as *const u32).read();
            if engine == 0 {
                return;
            }
            let Some(resource) = self.load() else {
                return;
            };
            let cue = 4096 + u32::from(id);
            let lookup: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(crate::memory::entry(0xb68280));
            if lookup(resource, cue) != 0 {
                let queue: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32) -> u32 =
                    core::mem::transmute(crate::memory::entry(0x4eeaa4));
                queue(engine, 7, resource, cue, actor, actor + 64);
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Clock {
    clip: u16,
    frame: f32,
}

impl Clock {
    /// Play authored cues crossed since the last sample. Each record contains
    /// a motion frame, bank number and up to four weighted cue alternatives.
    ///
    /// # Safety
    /// The actor must be a live hunter on GU's game thread.
    pub unsafe fn update(
        &mut self,
        actor: u32,
        banks: &[Bank],
        timeline: &[u8],
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
        let Some(&[a, b, c, d]) = timeline.as_chunks::<4>().0.get(index / 4) else {
            return;
        };
        let offset = usize::from(u16::from_le_bytes([a, b]));
        let count = usize::from(u16::from_le_bytes([c, d]));
        let Some(track) = timeline.get(offset..offset + 12 * count) else {
            return;
        };
        for row in track.chunks_exact(12) {
            let time = f32::from(u16::from_le_bytes([row[0], row[1]]));
            if time <= from || time > frame {
                continue;
            }
            let Some(bank) = banks.get(usize::from(row[2])) else {
                continue;
            };
            let Some(choices) = row.get(4..4 + 2 * usize::from(row[3])) else {
                continue;
            };
            let total: u32 = choices.chunks_exact(2).map(|c| u32::from(c[1])).sum();
            if total == 0 {
                continue;
            }
            let mut roll = if choices.len() > 2 {
                let rand: unsafe extern "C" fn() -> u32 =
                    unsafe { core::mem::transmute(crate::memory::entry(0x27c8f8)) };
                (unsafe { rand() }) % total
            } else {
                0
            };
            for c in choices.chunks_exact(2) {
                if roll < u32::from(c[1]) {
                    if c[0] != 255 {
                        unsafe { bank.play(actor, c[0]) };
                    }
                    break;
                }
                roll -= u32::from(c[1]);
            }
        }
    }
}
