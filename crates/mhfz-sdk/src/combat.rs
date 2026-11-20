//! Frontier SDT timing for blade contacts submitted to GU.

pub struct HitWindow {
    pub delay: f32,
    pub duration: f32,
}

impl HitWindow {
    /// A zero start is relative to the action event. Other rows use the clip frame.
    pub fn new(start: u16, length: u16, frame: f32, late: f32) -> Option<Self> {
        let until_start = if start == 0 {
            -late
        } else {
            f32::from(start) - frame
        };
        let duration = f32::from(length) + until_start.min(0.);
        (duration > 0.).then_some(Self {
            delay: until_start.max(0.),
            duration,
        })
    }
}
