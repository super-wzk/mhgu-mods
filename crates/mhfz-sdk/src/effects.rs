//! Frontier motion effect rows and their GU playback transforms.

#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod model;

use mhgu_sdk::style::action::MotionFrame;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
use mhgu_sdk::style::particle::{Anchor, ground_heights, world_offset};

pub struct MotionEvent {
    pub frame: f32,
    pub kind: u16,
    pub bank: u16,
    pub selector: u16,
    pub node: u8,
    pub mode: u8,
    pub offset: [f32; 3],
}

#[derive(Default)]
pub struct MotionClock {
    clip: u16,
    frame: f32,
}

impl MotionClock {
    pub fn seek(&mut self, clip: u16, frame: f32) {
        *self = Self { clip, frame };
    }

    /// Skip blended samples and split a loop into its tail and restarted head.
    pub fn advance<'a>(
        &mut self,
        rows: &'a [[u8; 18]],
        clip: u16,
        sample: MotionFrame,
    ) -> impl Iterator<Item = (MotionEvent, f32)> + 'a {
        let previous = self.frame;
        let same_clip = self.clip == clip;
        self.seek(clip, sample.frame);
        let ranges = if !same_clip || sample.blend > 0. {
            [(0., 0., 0.); 2]
        } else if sample.frame < previous
            && sample.restart > 0.
            && (sample.restart..sample.end).contains(&sample.frame)
        {
            [
                (previous, sample.end, sample.frame - sample.restart),
                (sample.restart, sample.frame, 0.),
            ]
        } else {
            [(previous, sample.frame, 0.), (0., 0., 0.)]
        };
        ranges
            .into_iter()
            .filter(|&(from, to, _)| from < to)
            .flat_map(move |(from, to, after)| {
                motion_events(rows, clip, from, to).map(move |event| {
                    let late = to - event.frame + after;
                    (event, late)
                })
            })
    }
}

/// FZ 817BD900 scans [previous, current) in the clip's authored event order.
fn motion_events(
    rows: &[[u8; 18]],
    clip: u16,
    previous: f32,
    current: f32,
) -> impl Iterator<Item = MotionEvent> + '_ {
    let start = rows.partition_point(|row| half(row, 0) < clip);
    rows[start..]
        .iter()
        .take_while(move |row| half(*row, 0) == clip)
        .filter_map(move |row| {
            let frame = f32::from(half(row, 2));
            (previous <= frame && frame < current).then(|| MotionEvent {
                frame,
                kind: half(row, 4),
                bank: half(row, 6),
                selector: half(row, 8),
                node: row[10],
                mode: row[11],
                offset: core::array::from_fn(|i| f32::from(half(row, 12 + i * 2) as i16)),
            })
        })
}

fn half(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

// FZ 810050CC decomposes normalized matrix columns in Z-Y-X order.
pub fn pose_angles(m: &[f32; 16]) -> [u16; 3] {
    let len = |i: usize| {
        libm::sqrtf(m[i] * m[i] + m[i + 1] * m[i + 1] + m[i + 2] * m[i + 2]).max(f32::EPSILON)
    };
    let xy = libm::hypotf(m[0], m[1]);
    let (x, z) = if xy < f32::EPSILON * len(0) {
        (libm::atan2f(-m[9] / len(8), m[5] / len(4)), 0.)
    } else {
        (
            libm::atan2f(m[6] / len(4), m[10] / len(8)),
            libm::atan2f(m[1], m[0]),
        )
    };
    [x, libm::atan2f(-m[2], xy), z].map(|r| (r * (65536. / core::f32::consts::TAU)) as i32 as u16)
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
/// # Safety
/// The actor must be a live GU hunter on the game thread.
pub unsafe fn motion_ground(actor: u32, e: &MotionEvent) -> Option<(Anchor, bool)> {
    unsafe {
        let (mut pos, yaw) = world_offset(actor, e.offset);
        let (floor, water) = ground_heights(actor, pos)?;
        pos[1] = water.unwrap_or(floor);
        let yaw = if e.mode == 4 {
            yaw.wrapping_add(0x4000)
        } else {
            yaw
        };
        Some((Anchor::World(pos, yaw), water.is_some()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_effects_keep_order_lateness_and_half_open_bounds() {
        let rows = [20u16, 22, 59, 60].map(|frame| {
            let mut row = [0u8; 18];
            row[..2].copy_from_slice(&1000u16.to_le_bytes());
            row[2..4].copy_from_slice(&frame.to_le_bytes());
            row
        });
        let sample = MotionFrame {
            frame: 22.,
            end: 60.,
            restart: 20.,
            blend: 0.,
            ended: false,
        };
        let mut clock = MotionClock::default();
        clock.seek(1000, 58.);
        let mut events = clock.advance(&rows, 1000, sample);
        assert_eq!(
            events.next().map(|(e, late)| (e.frame, late)),
            Some((59., 3.))
        );
        assert_eq!(
            events.next().map(|(e, late)| (e.frame, late)),
            Some((20., 2.))
        );
        assert!(events.next().is_none());
        assert!(clock.advance(&rows, 1000, sample).next().is_none());
        assert!(
            clock
                .advance(
                    &rows,
                    1000,
                    MotionFrame {
                        frame: 23.,
                        blend: 1.,
                        ..sample
                    }
                )
                .next()
                .is_none()
        );
        assert!(
            clock
                .advance(
                    &rows,
                    1000,
                    MotionFrame {
                        frame: 24.,
                        ..sample
                    }
                )
                .next()
                .is_none()
        );
    }
}
