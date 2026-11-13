//! Launch parameters for a GU directed shell.
use crate::geometry::Angle;

/// Per-spawn world coordinates, GU angles (65536 units per turn), and velocity per frame.
pub struct ShellLaunch {
    pub origin: [f32; 3],
    pub pitch: Angle,
    pub yaw: Angle,
    pub velocity: [f32; 3],
}
