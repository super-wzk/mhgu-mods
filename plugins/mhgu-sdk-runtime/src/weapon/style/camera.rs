//! Apply the registered style's requests during the GU quest camera update.
use super::local_entry;
use mhgu_sdk::{geometry::Bearing, sync::Shared, weapon::style::rules::CameraLens};

struct ProjectedFov {
    camera: u32,
    stock: f32,
    applied: f32,
}

static PROJECTED_FOV: Shared<Option<ProjectedFov>> = Shared::new(None);

unsafe fn quest_camera(camera: u32) -> bool {
    // Only uQuestCamera (constructor 1FA00) provides a hunter at +2140.
    unsafe { (camera as *const u32).read() == forge::mem::text_addr() + 0x1730d0c }
}

// Keep the adjusted FOV on uQuestCamera for projection and rendering. Restore
// the stock value before GU starts the next update and uses it for smoothing.
#[forge::hook(offset = 0x21a90)]
unsafe fn update(camera: u32) -> u32 {
    PROJECTED_FOV.with(|slot| {
        if slot
            .as_ref()
            .is_some_and(|previous| previous.camera == camera)
            && let Some(previous) = slot.take()
        {
            let field = (camera + 60) as *mut f32;
            unsafe {
                if quest_camera(camera) && field.read() == previous.applied {
                    field.write(previous.stock);
                }
            }
        }
    });
    original!(camera)
}

// 24E54 builds the orbit, then passes the eye and target through 26E64 to
// the native monster and terrain collision pass at 26934.
#[forge::hook(offset = 0x26e64)]
unsafe fn orbit_collision(camera: u32, flags: u32) -> u32 {
    unsafe {
        if flags == 3
            && quest_camera(camera)
            && ((camera + 152) as *const u32).read() == 1
            && [368, 560, 752, 1082, 1136, 2488, 2712, 2764]
                .into_iter()
                .all(|offset| ((camera + offset) as *const u8).read() == 0)
        {
            adjust_lens(camera);
        }
        original!(camera, flags)
    }
}

unsafe fn adjust_lens(camera: u32) {
    unsafe {
        let actor = ((camera + 2140) as *const u32).read();
        let Some(request) = local_entry(actor)
            .and_then(|entry| entry.rules())
            .and_then(|rules| rules.camera_lens)
        else {
            return;
        };
        let elapsed = ((camera + 28) as *const f32).read();
        let fov = (camera + 60) as *mut f32;
        let eye = (camera + 64) as *mut [f32; 3];
        let target = (camera + 96) as *mut [f32; 3];
        let [ex, ey, ez] = eye.read();
        let [tx, ty, tz] = target.read();
        let hunter_y = ((actor + 68) as *const f32).read();
        let stock = CameraLens {
            fov: fov.read(),
            radius: Bearing::between_xz((tx, tz), (ex, ez)).distance,
            eye_height: ey - hunter_y,
            target_height: ty - hunter_y,
        };
        let mut lens = stock;
        request(actor, elapsed, &mut lens);
        if lens.radius != stock.radius
            && stock.radius > 0.
            && stock.radius.is_finite()
            && lens.radius.is_finite()
        {
            let scale = lens.radius.max(1.) / stock.radius;
            eye.write([tx + (ex - tx) * scale, ey, tz + (ez - tz) * scale]);
        }
        if lens.eye_height != stock.eye_height && lens.eye_height.is_finite() {
            ((camera + 68) as *mut f32).write(hunter_y + lens.eye_height);
        }
        if lens.target_height != stock.target_height && lens.target_height.is_finite() {
            ((camera + 100) as *mut f32).write(hunter_y + lens.target_height);
        }
        if lens.fov != stock.fov && lens.fov.is_finite() {
            let applied = lens.fov.clamp(0.01, 179.99);
            fov.write(applied);
            PROJECTED_FOV.with(|slot| {
                *slot = Some(ProjectedFov {
                    camera,
                    stock: stock.fov,
                    applied,
                })
            });
        }
    }
}

// The default orbit update at 24E54 calls 255CC before processing recenter
// and before smoothing the requested pitch and yaw at 2531C and 25340.
#[forge::hook(offset = 0x255cc)]
unsafe fn orbit_input(camera: u32, configuration: u32) -> u32 {
    unsafe {
        if quest_camera(camera)
            && ((camera + 2712) as *const u8).read() == 0
            && ((camera + 1082) as *const u8).read() == 0
        {
            let actor = ((camera + 2140) as *const u32).read();
            if let Some(yaw) = local_entry(actor)
                .and_then(|entry| entry.rules())
                .and_then(|rules| rules.camera_yaw)
                .and_then(|request| u16::try_from(request(actor)).ok())
            {
                // GU stores the orbit angle from the target toward the eye.
                ((camera + 2540) as *mut u32).write(u32::from(yaw.wrapping_add(0x8000)));
            }
        }
        original!(camera, configuration)
    }
}

// 21A90 calls 898AC0 through the one-word branch at 1C334. Hook the
// destination so Forge's two-word trampoline leaves the next virtual intact.
#[forge::hook(offset = 0x898ac0)]
unsafe fn view(camera: u32) -> u32 {
    unsafe {
        if !quest_camera(camera) {
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
    forge::install_hook!(base, update);
    forge::install_hook!(base, orbit_input);
    forge::install_hook!(base, orbit_collision);
    forge::install_hook!(base, view);
}
