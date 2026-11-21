//! Apply a registered style's displacement during the GU quest camera update.
use super::local_entry;

// 21A90 calls 898AC0 through the one-word branch at 1C334. Hook the
// destination so Forge's two-word trampoline leaves the next virtual intact.
#[forge::hook(offset = 0x898ac0)]
unsafe fn view(camera: u32) -> u32 {
    unsafe {
        // Only uQuestCamera (constructor 1FA00) provides a hunter at +2140.
        if (camera as *const u32).read() != forge::mem::text_addr() + 0x1730d0c {
            return original!(camera);
        }
        let actor = ((camera + 2140) as *const u32).read();
        let Some(offset) = local_entry(actor)
            .and_then(|entry| entry.rules())
            .and_then(|rules| rules.camera_offset)
        else {
            return original!(camera);
        };
        let eye = (camera + 64) as *const [f32; 3];
        let elapsed = ((camera + 28) as *const f32).read();
        let offset = offset(actor, elapsed, eye);
        if offset == 0. {
            return original!(camera);
        }
        let eye_y = (camera + 68) as *mut f32;
        let target_y = (camera + 100) as *mut f32;
        let (eye, target) = (eye_y.read(), target_y.read());
        eye_y.write(eye + offset);
        target_y.write(target + offset);
        let result = original!(camera);
        eye_y.write(eye);
        target_y.write(target);
        result
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, view);
}
