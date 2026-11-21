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
    scanned_until: f32,
}

impl MotionClock {
    pub fn seek(&mut self, clip: u16, frame: f32) {
        *self = Self {
            clip,
            frame,
            scanned_until: frame,
        };
    }

    /// Scan from the evaluated pose up to the next playhead. A loop starts
    /// a new interval once GU has evaluated the restarted pose.
    pub fn advance<'a>(
        &mut self,
        rows: &'a [[u8; 18]],
        clip: u16,
        sample: MotionFrame,
    ) -> impl Iterator<Item = MotionEvent> + 'a {
        if self.clip != clip || sample.frame < self.frame {
            self.seek(clip, sample.frame);
        }
        self.frame = sample.frame;
        let from = sample.frame.max(self.scanned_until);
        let to = if sample.blend <= 0. && sample.next > from {
            self.scanned_until = sample.next;
            sample.next
        } else {
            from
        };
        motion_events(rows, clip, from, to)
    }
}

/// FZ 817BD900 scans [+544, +476) in the clip's authored event order.
fn motion_events(
    rows: &[[u8; 18]],
    clip: u16,
    from: f32,
    to: f32,
) -> impl Iterator<Item = MotionEvent> + '_ {
    let start = if from < to {
        rows.partition_point(|row| half(row, 0) < clip)
    } else {
        rows.len()
    };
    rows[start..]
        .iter()
        .take_while(move |row| half(*row, 0) == clip)
        .filter_map(move |row| {
            let frame = f32::from(half(row, 2));
            (from <= frame && frame < to).then(|| MotionEvent {
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

/// Preserve the sampled bone basis when submitting a GU world particle.
/// FZ 810050CC and GU 8A7DC4 use different Euler orders. GU builds Ry * Rx * Rz.
pub fn world_pose_angles(pose: &[f32; 16]) -> [u16; 3] {
    let normalize = |i: usize| {
        let [x, y, z] = [pose[i], pose[i + 1], pose[i + 2]];
        let len = libm::sqrtf(x * x + y * y + z * z).max(f32::EPSILON);
        [x / len, y / len, z / len]
    };
    let [xx, xy, xz] = normalize(0);
    let [_, yy, _] = normalize(4);
    let [zx, zy, zz] = normalize(8);
    let horizontal = libm::hypotf(zx, zz);
    let (yaw, roll) = if horizontal > f32::EPSILON {
        (libm::atan2f(zx, zz), libm::atan2f(xy, yy))
    } else {
        (libm::atan2f(-xz, xx), 0.)
    };
    [libm::atan2f(-zy, horizontal), yaw, roll]
        .map(|r| (r * (65536. / core::f32::consts::TAU)) as i32 as u16)
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
    fn world_particle_angles_preserve_a_scaled_pose() {
        // FZ local X/Y/Z angles 20/35/50 degrees with column scales 2/3/4.
        #[rustfmt::skip]
        let pose = [
             1.0530816, 1.2550137, -1.1471529, 0.,
            -1.7812429, 2.2629039,  0.8404995, 0.,
             2.4338234, 0.7721623,  3.0790045, 0.,
             0.,        0.,         0.,        1.,
        ];
        for (actual, expected) in world_pose_angles(&pose)
            .into_iter()
            .zip([63510, 6976, 7237])
        {
            assert!((actual.wrapping_sub(expected) as i16).abs() <= 1);
        }
        // At +90 degrees of pitch the same basis is represented with zero roll.
        #[rustfmt::skip]
        let vertical = [
             0.,  0., -1., 0.,
             1.,  0.,  0., 0.,
             0., -1.,  0., 0.,
             0.,  0.,  0., 1.,
        ];
        assert_eq!(world_pose_angles(&vertical), [0x4000, 0x4000, 0]);
    }

    #[test]
    fn events_use_the_pose_and_exclude_the_next_playhead() {
        let rows = [20u16, 22, 50, 51, 59, 60, 84].map(|frame| {
            let mut row = [0u8; 18];
            row[..2].copy_from_slice(&1448u16.to_le_bytes());
            row[2..4].copy_from_slice(&frame.to_le_bytes());
            row
        });
        let mut clock = MotionClock::default();
        clock.seek(1448, 48.);
        let mut check = |frame, next, blend, expected: &[u16]| {
            let events = clock
                .advance(
                    &rows,
                    1448,
                    MotionFrame {
                        frame,
                        next,
                        end: 100.,
                        restart: 20.,
                        blend,
                        ended: false,
                    },
                )
                .map(|event| event.frame as u16);
            assert!(events.eq(expected.iter().copied()), "{frame}..{next}");
        };
        check(48., 50., 0., &[]);
        check(50., 52., 0., &[50, 51]);
        check(50., 52., 0., &[]);
        check(58., 60., 0., &[59]);
        check(60., 20., 0., &[]);
        check(20., 20., 0., &[]);
        check(20., 22., 0., &[20]);
        check(22., 23., 0., &[22]);
        check(82., 84., 2., &[]);
        check(84., 85., 0., &[84]);
    }
}
