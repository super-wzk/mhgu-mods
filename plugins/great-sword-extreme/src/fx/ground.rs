use super::{Attachment, field, joint_matrix, route};
use mhgu_sdk::geometry::Angle;

const SURFACE: usize = 16;
const DEBRIS: usize = 17;
const CHARGED_IMPACT: usize = 18;

unsafe fn impact_origin(p: u32, side: f32, forward: f32) -> [f32; 3] {
    unsafe {
        let [x, y, z]: [f32; 3] = field(p, 64);
        let (sin, cos) = Angle::from_units(field(p, 4076)).sin_cos();
        [
            x + side * cos + forward * sin,
            y + 15.,
            z - side * sin + forward * cos,
        ]
    }
}

// GU's floor query also records the highest special surface at this X/Z.
// Use a fresh hit record rather than the floor cached under the hunter.
unsafe fn heights(p: u32, position: [f32; 3]) -> Option<(f32, Option<f32>)> {
    unsafe {
        let base = forge::mem::text_addr();
        let manager = field::<u32>(base, 0x21270e0);
        if manager == 0 {
            return None;
        }
        let mut hit = [0u32; 12];
        let pos = [position[0], position[1], position[2], 0.];
        let mut floor = 0f32;
        let query: unsafe extern "C" fn(
            u32,
            u32,
            *const f32,
            *const f32,
            u32,
            *mut f32,
            *mut u32,
            u32,
            u32,
            u32,
        ) -> u32 = core::mem::transmute(base + 0x18192c);
        if query(
            manager,
            p,
            pos.as_ptr(),
            pos.as_ptr(),
            u32::from(field::<u8>(p, 4180)),
            &mut floor,
            hit.as_mut_ptr(),
            0,
            0,
            0,
        ) != 1
        {
            return None;
        }
        let surface = f32::from_bits(hit[5]);
        Some((
            floor,
            (surface > -100000. && surface >= floor - 1.).then_some(surface),
        ))
    }
}

unsafe fn impact(p: u32, mut pos: [f32; 3], dry: usize) {
    unsafe {
        let wet = heights(p, pos).and_then(|(_, surface)| surface);
        let selector = if let Some(y) = wet
            && y >= pos[1] - 18.
        {
            pos[1] = y + 5.;
            SURFACE
        } else {
            dry
        };
        route(p, selector, Attachment::World(pos, field(p, 4076)));
    }
}

pub(super) unsafe fn uppercut_burst(p: u32, dry_route: usize) {
    unsafe {
        impact(p, impact_origin(p, 49., 86.), dry_route);
    }
}

pub(crate) unsafe fn ground_strike(p: u32, kind: u16, level: u8) {
    unsafe {
        if kind != 25 || level != 3 {
            let pos = if kind == 26 {
                impact_origin(p, 50., -150.)
            } else {
                impact_origin(p, 0., 150.)
            };
            impact(p, pos, DEBRIS);
            return;
        }
        // FZ 811D3B3C samples node 14 along (1,0,1). GU has the
        // standard great sword reach, corresponding to FZ reach entry 0.
        let Some(joint) = joint_matrix(p, 8) else {
            return;
        };
        let m: [f32; 16] = field(joint, 0);
        let blade =
            |x, z| -> [f32; 3] { core::array::from_fn(|i| m[12 + i] + x * m[i] + z * m[8 + i]) };
        let yaw = field(p, 4076);
        if let Some((_, Some(surface))) = heights(p, blade(160., 160.)) {
            for distance in [60., 110., 160., 210., 260.] {
                let mut pos = blade(distance, distance);
                pos[1] = surface;
                route(p, SURFACE, Attachment::World(pos, yaw));
            }
        } else {
            let mut pos = blade(44., 123.);
            if let Some((floor, _)) = heights(p, pos) {
                pos[1] = floor;
            }
            route(p, CHARGED_IMPACT, Attachment::World(pos, yaw));
        }
    }
}
