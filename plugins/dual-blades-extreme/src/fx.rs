//! Frontier effects played by GU's native particle manager.
use crate::{native, read, write};
use core::sync::atomic::{AtomicU32, Ordering};
mod trail;
pub(super) use trail::install;

static PEL: AtomicU32 = AtomicU32::new(0);
const ROUTES: &[u8; 60] = include_bytes!("../target/assets/effects/routes.bin");
const BLADE_JOINTS: [u8; 2] = [8, 12];
const SHARPEN_SPARK: usize = 1;
const SHARPEN_FLASH: usize = 4;
const SHARPEN_SLASH: usize = 5;
const DEMON_GLOW: usize = 6;
const DEMON_TRAIL: usize = 9;
const DEMON_PHASES: usize = 3;
const DEMON_PERIOD: f32 = 12.;

#[derive(Default)]
pub(super) struct Effects {
    phase: u8,
    until_demon: f32,
    spark: usize,
    history: trail::History,
}

impl Effects {
    pub(super) unsafe fn demon(&mut self, actor: u32, phase: u8, dt: f32) {
        unsafe {
            self.history.sample(actor, phase, dt);
        }
        if phase != self.phase {
            self.phase = phase;
            self.until_demon = 0.;
        } else if dt <= 0. {
            return;
        }
        self.until_demon = (self.until_demon - dt).max(0.);
        if phase == 0 || self.until_demon > 0. {
            return;
        }
        unsafe {
            let Some(parent) = particle_parent(actor) else {
                return;
            };
            let phase_index = usize::from(phase - 1);
            for (hand, joint) in BLADE_JOINTS.into_iter().enumerate() {
                emit(
                    actor,
                    DEMON_GLOW + phase_index,
                    Anchor::Joint { parent, joint },
                );
                let route = DEMON_TRAIL + hand * DEMON_PHASES + phase_index;
                emit(actor, route, Anchor::Trail(read(actor, 64)));
            }
        }
        // The final two 30 Hz keys overlap the next emission as they fade out.
        self.until_demon = DEMON_PERIOD;
    }

    pub(super) unsafe fn sharpen_slash(&mut self, actor: u32, ticks: u16) {
        unsafe {
            self.history.sharpen(actor, self.phase, ticks);
            emit(actor, SHARPEN_SLASH, Anchor::Trail(read(actor, 64)));
        }
    }

    pub(super) unsafe fn sharpen(&mut self, actor: u32, first: bool) {
        unsafe {
            if let Some(matrix) = joint_matrix(actor, BLADE_JOINTS[0]) {
                emit(
                    actor,
                    SHARPEN_SPARK + self.spark,
                    Anchor::World(read(matrix, 48)),
                );
                self.spark = (self.spark + 1) % 3;
            }
            if first && let Some(parent) = particle_parent(actor) {
                emit(
                    actor,
                    SHARPEN_FLASH,
                    Anchor::Joint {
                        parent,
                        joint: BLADE_JOINTS[0],
                    },
                );
            }
        }
    }
}

unsafe fn particle_parent(actor: u32) -> Option<u32> {
    unsafe {
        if read::<u32>(actor, 1172) == 0 || read::<u32>(actor, 1176) == 0 {
            return None;
        }
        let get: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(read::<u32>(read(actor, 0), 304));
        let parent = get(actor);
        (parent != 0).then_some(parent)
    }
}

unsafe fn joint_matrix(actor: u32, joint: u8) -> Option<u32> {
    unsafe {
        if read::<u32>(actor, 1172) == 0 || read::<u32>(actor, 1176) == 0 {
            return None;
        }
        let matrix = native!(0xc1628, actor, u32::from(joint));
        (matrix != 0 && matrix != actor + 176).then_some(matrix)
    }
}

enum Anchor {
    World([f32; 3]),
    Trail([f32; 3]),
    Joint { parent: u32, joint: u8 },
}

unsafe fn resource(base: u32) -> u32 {
    unsafe {
        let cached = PEL.load(Ordering::Acquire);
        if cached != 0 {
            return cached;
        }
        let manager = read::<u32>(base, 0x2126a64);
        if manager == 0 {
            return 0;
        }
        let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
            core::mem::transmute(read::<u32>(read(manager, 0), 48));
        let resource = load(
            manager,
            base + 0x1889fdc,
            c"effect\\frontier_db\\particles".as_ptr(),
            1,
        );
        if resource == 0 {
            return 0;
        }
        if read::<u32>(resource, 80) & 1 == 0
            || PEL
                .compare_exchange(0, resource, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            native!(0x887660, resource);
        }
        PEL.load(Ordering::Acquire)
    }
}

unsafe fn emit(actor: u32, route: usize, anchor: Anchor) {
    unsafe {
        let [lo, hi, count_lo, count_hi] = ROUTES.as_chunks::<4>().0[route];
        let first = u16::from_le_bytes([lo, hi]);
        let count = u16::from_le_bytes([count_lo, count_hi]);
        let base = forge::mem::text_addr();
        let manager = read::<u32>(base, 0x188a0d4);
        if manager == 0 || read::<u32>(base, 0x2126f48) == 0 {
            return;
        }
        let pel = resource(base);
        if pel == 0 {
            return;
        }
        let handle = native!(0x3268d8, manager, pel, 0);
        if handle == 0 || read::<u32>(handle, 4) == u32::MAX {
            return;
        }
        for selector in first..first + count {
            let unit = native!(0x3288c4, 416, 16);
            if unit == 0 {
                continue;
            }
            native!(0x328900, unit);
            let proof = unit + 128;
            match anchor {
                Anchor::World(position) | Anchor::Trail(position) => {
                    let yaw = if matches!(anchor, Anchor::Trail(_)) {
                        0.
                    } else {
                        f32::from(read::<u16>(actor, 4076)) * 360. / 65536.
                    };
                    write(proof, 4, 0xau32);
                    write(proof, 12, 1u32);
                    write(proof, 70, 3u8);
                    write(proof, 176, position);
                    write(proof, 32, [0., yaw, 0.]);
                }
                Anchor::Joint { parent, joint } => {
                    write(proof, 4, 0x28u32);
                    write(proof, 12, 2u32);
                    write(proof, 66, i16::from(joint));
                    write(proof, 70, 0u8);
                    write(proof, 192, parent);
                }
            }
            if native!(
                0x3267d0,
                manager,
                unit,
                read::<u32>(handle, 4),
                1,
                u32::from(selector),
                0
            ) == 0
            {
                let destroy: unsafe extern "C" fn(u32) =
                    core::mem::transmute(read::<u32>(read(unit, 0), 4));
                destroy(unit);
            }
        }
        native!(0x326c78, manager, handle, 0);
    }
}

pub(super) unsafe fn evade(actor: u32) {
    unsafe {
        let [x, y, z] = read::<[f32; 3]>(actor, 64);
        // FZ 811EE130 translates the scene root by (0, 90, 130).
        let (sin, cos) = mhgu_sdk::geometry::Angle::from_units(read(actor, 4076)).sin_cos();
        emit(
            actor,
            0,
            Anchor::World([x + 130. * sin, y + 90., z + 130. * cos]),
        );
    }
}
