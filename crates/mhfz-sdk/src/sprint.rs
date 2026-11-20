//! Frontier sprint input, braking and resume policy.

use mhgu_sdk::style::Input;

pub const STICK_DEADZONE: i16 = 48;

#[derive(Default)]
pub struct Flick([Candidate; 3]);

#[derive(Default)]
enum Phase {
    #[default]
    Neutral,
    FirstPush,
    Release,
    SecondPush,
    CorrectDirection,
}

#[derive(Default)]
struct Candidate {
    phase: Phase,
    direction: u16,
    strength: i16,
    remaining: f32,
}

impl Flick {
    pub fn update(&mut self, input: &Input, elapsed: f32) -> bool {
        // FZ 81ECC418 tracks three overlapping candidates. Only one may
        // record a first push on a sample, and success clears all three.
        let mut first_push_available = true;
        for candidate in &mut self.0 {
            if candidate.update(input, elapsed, &mut first_push_available) {
                *self = Self::default();
                return true;
            }
        }
        false
    }
}

impl Candidate {
    fn update(&mut self, input: &Input, elapsed: f32, first_push_available: &mut bool) -> bool {
        use Phase::*;
        // FZ's 12- and 6-tick windows in GU's 60 Hz elapsed frames.
        const WINDOW: f32 = 24.;
        const CORRECTION: f32 = 12.;
        const PUSH: i16 = 80;
        let strength = input.stick_strength;
        let angle = input.stick_angle;
        let neutral = strength < STICK_DEADZONE;
        if matches!(self.phase, Neutral) && neutral {
            self.phase = FirstPush;
        }
        if matches!(self.phase, FirstPush) && strength >= PUSH {
            if !*first_push_available {
                self.phase = Neutral;
                return false;
            }
            *first_push_available = false;
            self.phase = Release;
            self.direction = angle;
            self.strength = strength;
            self.remaining = WINDOW;
        }
        let delta = angle.wrapping_sub(self.direction);
        let reversed = (0x7000..=0x9000).contains(&delta);
        let near = delta <= 0x3000 || delta >= 0xd000;
        if matches!(self.phase, Release) {
            if !neutral && !reversed {
                if strength > self.strength {
                    self.strength = strength;
                    self.direction = angle;
                }
                if near {
                    self.wait(elapsed, Neutral);
                } else {
                    self.phase = Neutral;
                }
                return false;
            }
            self.phase = SecondPush;
            self.remaining = WINDOW;
        }
        if matches!(self.phase, SecondPush) {
            if strength < PUSH || reversed {
                self.wait(elapsed, if neutral { FirstPush } else { Neutral });
                return false;
            }
            self.phase = CorrectDirection;
            self.remaining = CORRECTION;
        }
        if matches!(self.phase, CorrectDirection) {
            if neutral {
                self.phase = FirstPush;
            } else if strength < PUSH {
                self.wait(elapsed, Neutral);
            } else if delta <= 0x1000 || delta >= 0xe000 {
                return true;
            } else if near {
                self.wait(elapsed, Neutral);
            } else {
                self.phase = Neutral;
            }
        }
        false
    }

    fn wait(&mut self, elapsed: f32, expired: Phase) {
        if self.remaining > 0. {
            self.remaining = (self.remaining - elapsed).max(0.);
        } else if elapsed > 0. {
            self.phase = expired;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(samples: &[(i16, u16)], expected: &[bool]) {
        let mut flick = Flick::default();
        for (&(strength, angle), &starts) in samples.iter().zip(expected) {
            let input = Input {
                common: [0; 4],
                weapon: [0; 4],
                stick_angle: angle,
                stick_strength: strength,
                auxiliary_pressed: 0,
            };
            assert_eq!(
                flick.update(&input, 2.),
                starts,
                "strength={strength}, angle={angle:#x}"
            );
        }
    }

    #[test]
    fn second_push_and_first_push_turn_match_frontier() {
        replay(
            &[(0, 0), (100, 0), (0, 0), (100, 0)],
            &[false, false, false, true],
        );
        replay(
            &[(0, 0), (100, 0), (0, 0), (100, 0x2000), (100, 0)],
            &[false, false, false, false, true],
        );
        replay(
            &[(0, 0), (100, 0), (100, 0x4000), (0, 0), (100, 0)],
            &[false; 5],
        );
    }
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native {
    use mhgu_sdk::style::{action, movement::CLIMBS};

    pub fn can_brake(frame: f32, cycles: u16) -> bool {
        frame >= 30. || cycles != 0
    }

    fn terrain((group, slot): (u16, u16)) -> bool {
        (group == 1 && CLIMBS.contains(&slot)) || matches!((group, slot), (4, 3 | 14 | 23))
    }

    #[derive(Default)]
    pub struct Resume(bool);

    impl Resume {
        pub fn set(&mut self, pending: bool) {
            self.0 = pending;
        }

        /// Track a GU climb/fall/landing until the hunter can sprint again.
        ///
        /// # Safety
        /// The actor must be a live hunter on the game thread.
        pub unsafe fn on_native(
            &mut self,
            actor: u32,
            current: (u16, u16),
            ground: bool,
            drawn: bool,
            was_sprinting: bool,
        ) {
            let crossing = terrain(current);
            if was_sprinting {
                self.0 = crossing || (ground && terrain(unsafe { action::previous(actor) }));
            }
            self.0 &= drawn && (ground || crossing);
        }

        pub fn ready(&mut self, stick_held: bool, has_stamina: bool) -> bool {
            self.0 &= stick_held && has_stamina;
            self.0
        }
    }
}
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use native::{Resume, can_brake};
