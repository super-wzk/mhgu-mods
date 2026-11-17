//! Native movement posture, area, position and heading.
use super::Posture;
use crate::geometry::{Angle, Bearing};
use crate::{
    memory::{Field, entry},
    monster::{Monster, MonsterState, MonsterUpdate},
};

// BC8B0 writes POSTURE; BCB9C's separate +704 field is not a posture.
const POSTURE: Field<u8> = Field::at(442);
const AREA: Field<u8> = Field::at(4180);
const POSITION: Field<[f32; 3]> = Field::at(64);
const HEIGHT: Field<f32> = Field::at(68);
// BF828: the extra display height for posture 3, in model units.
const HOVER_HEIGHT: Field<f32> = Field::at(460);
const BASE_SCALE: Field<f32> = Field::at(428);
const SIZE_SCALE: Field<f32> = Field::at(432);
const GROUND_HEIGHT: Field<f32> = Field::at(1460);
const YAW: Field<u32> = Field::at(84);
const SET_POSTURE: u32 = 0xbc8b0;

impl Monster<'_> {
    pub fn area(self) -> u8 {
        self.0.get(AREA)
    }
}
impl MonsterUpdate<'_> {
    /// Move to world-space flight height without dropping the native hover offset.
    pub fn enter_free_flight(self) {
        let Some(state) = self.monster().state() else {
            return;
        };
        match state.posture() {
            Posture::Airborne => return,
            Posture::Flight => {
                let scale = state.0.get(BASE_SCALE) * state.0.get(SIZE_SCALE);
                state.set_height(state.height() + state.0.get(HOVER_HEIGHT) * scale);
            }
            _ => {}
        }
        self.set_posture(Posture::Airborne);
    }

    /// Change to a supported native posture during an action update.
    pub fn set_posture(self, posture: Posture) {
        if !matches!(
            posture,
            Posture::Flight | Posture::Ground | Posture::Airborne
        ) || self.monster().state().is_none()
        {
            return;
        }
        let change = unsafe {
            core::mem::transmute::<usize, unsafe extern "C" fn(u32, u32) -> u32>(entry(SET_POSTURE))
        };
        unsafe {
            change(self.monster().as_raw(), posture.raw().into());
        }
    }
}

impl MonsterState<'_> {
    pub fn posture(self) -> Posture {
        self.0.get(POSTURE).into()
    }
    pub fn position(self) -> [f32; 3] {
        self.0.get(POSITION)
    }
    pub fn set_position(self, xyz: [f32; 3]) {
        self.0.set(POSITION, xyz);
    }
    pub fn height(self) -> f32 {
        self.0.get(HEIGHT)
    }
    pub fn set_height(self, y: f32) {
        self.0.set(HEIGHT, y);
    }
    pub fn ground_height(self) -> f32 {
        self.0.get(GROUND_HEIGHT)
    }
    /// Horizontal heading: 65536 units per turn. The native slot is four bytes wide.
    pub fn yaw(self) -> Angle {
        Angle::from_units(self.0.get(YAW) as u16)
    }
    pub fn set_yaw(self, yaw: Angle) {
        self.0.set(YAW, u32::from(yaw.units()));
    }
    pub fn bearing_to(self, point: (f32, f32)) -> Bearing {
        let [x, _, z] = self.position();
        Bearing::between_xz((x, z), point)
    }
    pub fn turn_toward(self, goal: Angle, max_units: u16) {
        self.set_yaw(self.yaw().step_toward(goal, max_units));
    }
}
