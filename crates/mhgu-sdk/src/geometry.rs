//! GU angles use 65536 units per turn; yaw zero faces +Z, positive yaw turns toward +X.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Angle(u16);

impl Angle {
    pub const fn from_units(units: u16) -> Self {
        Self(units)
    }
    pub const fn units(self) -> u16 {
        self.0
    }
    pub fn radians(self) -> f32 {
        f32::from(self.0) * (core::f32::consts::TAU / 65536.)
    }
    pub fn sin_cos(self) -> (f32, f32) {
        libm::sincosf(self.radians())
    }
    pub const fn offset(self, units: i32) -> Self {
        Self(self.0.wrapping_add(units as u16))
    }
    /// Signed shortest turn; an exact half turn follows GU's negative i16 direction.
    pub const fn delta_to(self, target: Self) -> i16 {
        target.0.wrapping_sub(self.0) as i16
    }
    pub fn step_toward(self, target: Self, max_units: u16) -> Self {
        let limit = i32::from(max_units);
        self.offset(i32::from(self.delta_to(target)).clamp(-limit, limit))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bearing {
    pub distance: f32,
    pub yaw: Angle,
}
impl Bearing {
    pub fn between_xz(from: (f32, f32), to: (f32, f32)) -> Self {
        let (dx, dz) = (to.0 - from.0, to.1 - from.1);
        Self {
            distance: libm::sqrtf(dx * dx + dz * dz),
            yaw: Angle::from_units(
                (libm::atan2f(dx, dz) * (65536. / core::f32::consts::TAU)) as i32 as u16,
            ),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gu_axes_and_shortest_turn() {
        let from = (11., -7.);
        for (to, distance, units) in [
            ((11., -3.), 4., 0),
            ((14., -7.), 3., 16384),
            ((11., -9.), 2., 32768),
            ((10., -7.), 1., 49152),
        ] {
            let b = Bearing::between_xz(from, to);
            assert_eq!((b.distance, b.yaw.units()), (distance, units));
        }
        let a = Angle::from_units(65530);
        let b = Angle::from_units(5);
        assert_eq!(a.delta_to(b), 11);
        assert_eq!(a.step_toward(b, 8).units(), 2);
        assert_eq!(b.step_toward(a, 20), a);
        assert_eq!(a.delta_to(a.offset(32768)), i16::MIN);
        assert_eq!(a.step_toward(b, 0), a);
    }
}
