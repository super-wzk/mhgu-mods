//! Commands consumed by GU's native damage action interpreter (A134C / 9EF3C).
use crate::monster::motion::MotionId;
use alloc::boxed::Box;

/// Supported completion tests in A0D10. SetTimer initializes the TimerExpired clock.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum DamageMotionEnd {
    MotionFinished = 1,
    TimerExpired = 4,
}

pub enum DamageStep {
    Play {
        motion: MotionId,
        blend: f32,
        until: DamageMotionEnd,
    },
    SetTimer(u32),
}

const STOP: [u32; 2] = [255, 0];
const PLAY: u32 = 0;
const SET_TIMER: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MotionDescriptor {
    motion: u32,
    blend: f32,
    start_frame: f32,
    flags: u32,
    end_condition: u32,
    parameters: u32,
}

// Moving the Box never moves the descriptors referenced by play commands.
// The extra stop command immediately follows the last usable command slot.
#[repr(C)]
struct Storage<const N: usize> {
    commands: [[u32; 2]; N],
    stop: [u32; 2],
    motions: [MotionDescriptor; N],
}

/// Fixed-address GU program. Keep the owner alive while GU retains the program and
/// replace its contents only at entry to a new damage action, on the game thread.
pub struct DamageProgram<const N: usize>(Box<Storage<N>>);

impl<const N: usize> Default for DamageProgram<N> {
    fn default() -> Self {
        Self(Box::new(Storage {
            commands: [STOP; N],
            stop: STOP,
            motions: [MotionDescriptor::default(); N],
        }))
    }
}

impl<const N: usize> DamageProgram<N> {
    /// Encode a complete sequence. On failure leave the previous program untouched.
    /// GU's command index is one byte, including the index of the terminator.
    pub fn replace(&mut self, steps: &[DamageStep]) -> bool {
        if steps.len() > N
            || steps.len() > usize::from(u8::MAX)
            || steps
                .iter()
                .any(|s| matches!(s, DamageStep::Play { blend, .. } if !blend.is_finite()))
        {
            return false;
        }
        for (i, step) in steps.iter().enumerate() {
            self.0.commands[i] = match *step {
                DamageStep::Play {
                    motion,
                    blend,
                    until,
                } => {
                    let descriptor = &mut self.0.motions[i];
                    *descriptor = MotionDescriptor {
                        motion: motion.raw().into(),
                        blend,
                        end_condition: until as u32,
                        ..MotionDescriptor::default()
                    };
                    [PLAY, core::ptr::from_mut(descriptor) as u32]
                }
                DamageStep::SetTimer(frames) => [SET_TIMER, frames],
            };
        }
        if let Some(end) = self.0.commands.get_mut(steps.len()) {
            *end = STOP;
        }
        true
    }

    /// For GU's native damage callback; the program contains absolute guest addresses.
    pub fn as_ptr(&self) -> *const u32 {
        (&raw const *self.0).cast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_or_rewriting_owner_keeps_program_and_descriptor_at_fixed_addresses() {
        let mut program = DamageProgram::<1>::default();
        let start = program.as_ptr();
        for until in [
            DamageMotionEnd::MotionFinished,
            DamageMotionEnd::TimerExpired,
        ] {
            assert!(program.replace(&[DamageStep::Play {
                motion: MotionId::new(4, 1),
                blend: 2.,
                until
            }]));
            let moved = program;
            assert_eq!(moved.as_ptr(), start);
            let descriptor = &moved.0.motions[0];
            assert_eq!(
                moved.0.commands[0],
                [0, core::ptr::from_ref(descriptor) as u32]
            );
            assert_eq!(descriptor.motion, 0x401);
            assert_eq!(descriptor.blend, 2.);
            assert_eq!(descriptor.end_condition, until as u32);
            program = moved;
        }
    }

    #[test]
    fn rejects_oversized_program_without_replacing_the_active_one() {
        let mut program = DamageProgram::<1>::default();
        assert!(program.replace(&[DamageStep::SetTimer(240)]));
        assert!(!program.replace(&[DamageStep::SetTimer(1), DamageStep::SetTimer(2)]));
        assert!(!program.replace(&[DamageStep::Play {
            motion: MotionId::new(4, 1),
            blend: f32::NAN,
            until: DamageMotionEnd::MotionFinished
        }]));
        assert_eq!(program.0.commands[0], [3, 240]);
        assert_eq!(program.0.stop, [255, 0]);
        // The native cursor is u8 even when the owner offers more storage.
        let mut large = DamageProgram::<256>::default();
        let steps: [DamageStep; 256] = core::array::from_fn(|_| DamageStep::SetTimer(1));
        assert!(!large.replace(&steps));
        assert!(large.replace(&steps[..255]));
        assert_eq!(large.0.commands[255], [255, 0]);
    }
}
