use super::{Attachment, field, route};
use mhgu_sdk::{
    geometry::Angle,
    weapon::style::particle::{ground_heights, joint_matrix},
};

const SURFACE: usize = 16;
const DEBRIS: usize = 17;
const CHARGED_IMPACT: usize = 18;

unsafe fn ground_burst(p: u32, side: f32, forward: f32, dry_route: usize) {
    unsafe {
        let [x, y, z]: [f32; 3] = field(p, 64);
        let (sin, cos) = Angle::from_units(field(p, 4076)).sin_cos();
        let mut pos = [
            x + side * cos + forward * sin,
            y + 15.,
            z - side * sin + forward * cos,
        ];
        let selector = if let Some((_, Some(surface))) = ground_heights(p, pos)
            && surface >= pos[1] - 18.
        {
            pos[1] = surface + 5.;
            SURFACE
        } else {
            dry_route
        };
        route(p, selector, Attachment::World(pos, field(p, 4076)));
    }
}

pub(super) unsafe fn uppercut_burst(p: u32, dry_route: usize) {
    unsafe {
        ground_burst(p, 49., 86., dry_route);
    }
}

pub(crate) unsafe fn ground_strike(p: u32, kind: u16, level: u8) {
    unsafe {
        if kind != 25 || level != 3 {
            let (side, forward) = if kind == 26 { (50., -150.) } else { (0., 150.) };
            ground_burst(p, side, forward, DEBRIS);
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
        if let Some((_, Some(surface))) = ground_heights(p, blade(160., 160.)) {
            for distance in [60., 110., 160., 210., 260.] {
                let mut pos = blade(distance, distance);
                pos[1] = surface;
                route(p, SURFACE, Attachment::World(pos, yaw));
            }
        } else {
            let mut pos = blade(44., 123.);
            if let Some((floor, _)) = ground_heights(p, pos) {
                pos[1] = floor;
            }
            route(p, CHARGED_IMPACT, Attachment::World(pos, yaw));
        }
    }
}
