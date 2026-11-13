//! Ordinary Espinas spawn, sleep and wake transitions.

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum SpawnPhase {
    Enter,
    LyingDown,
    Sleeping,
    Rising,
    AfterRise,
    Awake,
}

impl SpawnPhase {
    /// The same clip is used when entering this phase or resuming it after an interruption.
    pub fn motion(self) -> Option<u8> {
        match self {
            Self::LyingDown => Some(39),
            Self::Sleeping => Some(34),
            Self::Rising => Some(37),
            Self::AfterRise => Some(62),
            Self::Enter | Self::Awake => None,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Step {
    Keep,
    Play(u8),
    Think,
}

pub struct Spawn {
    pub phase: SpawnPhase,
    initial_hp: i32,
    remaining: u16,
    special_wake: bool,
}

impl Spawn {
    // sub_81CC54EE: the ordinary quest branch stores HP and disables thinking
    // before dispatching 0:40. An explicit awake spawn is a distinct branch.
    pub fn new(hp: i32, awake: bool) -> Self {
        Self {
            phase: if awake {
                SpawnPhase::Awake
            } else {
                SpawnPhase::Enter
            },
            initial_hp: hp,
            remaining: 9000,
            special_wake: false,
        }
    }

    pub fn advance(
        &mut self,
        hp: i32,
        elapsed_ticks: u16,
        motion_finished: bool,
        special_wake: bool,
    ) -> Step {
        self.remaining = self.remaining.saturating_sub(elapsed_ticks);
        self.phase = match self.phase {
            SpawnPhase::Enter => SpawnPhase::LyingDown,
            SpawnPhase::LyingDown if motion_finished => SpawnPhase::Sleeping,
            SpawnPhase::Sleeping => {
                // 81CC6254: ordinary (non-HC) threshold is a strict comparison;
                // neither a hunter entering the area nor every positive hit wakes it.
                let damage_wake = self.initial_hp - hp > (self.initial_hp * 2 / 100).min(1000);
                if !damage_wake && self.remaining != 0 && !special_wake {
                    return Step::Keep;
                }
                self.special_wake = special_wake && !damage_wake;
                SpawnPhase::Rising
            }
            SpawnPhase::Rising if motion_finished && !self.special_wake => SpawnPhase::AfterRise,
            SpawnPhase::Rising | SpawnPhase::AfterRise if motion_finished => SpawnPhase::Awake,
            SpawnPhase::Awake => return Step::Think,
            _ => return Step::Keep,
        };
        self.phase.motion().map_or(Step::Think, Step::Play)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_spawn_waits_through_chip_damage_then_finishes_get_up() {
        let mut b = Spawn::new(5000, false);
        assert_eq!(b.advance(5000, 0, false, false), Step::Play(39));
        assert_eq!(b.advance(5000, 1, true, false), Step::Play(34));
        assert_eq!(b.advance(4900, 600, true, false), Step::Keep);
        assert_eq!(b.advance(4899, 1, false, false), Step::Play(37));
        assert_eq!(b.advance(4899, 1, false, false), Step::Keep);
        assert_eq!(b.advance(4899, 1, true, false), Step::Play(62));
        assert_eq!(b.advance(4899, 1, true, false), Step::Think);
    }
    #[test]
    fn timer_and_quest_override_are_independent() {
        let mut b = Spawn::new(30000, false);
        b.advance(30000, 0, false, false);
        b.advance(30000, 1, true, false);
        assert_eq!(b.advance(30000, 8998, false, false), Step::Keep);
        assert_eq!(b.advance(30000, 1, false, false), Step::Play(37));
        assert_eq!(
            Spawn::new(30000, true).advance(30000, 0, false, false),
            Step::Think
        );
    }
    #[test]
    fn damage_above_the_cap_keeps_the_full_wake_sequence_even_on_a_special_wake() {
        for (hp, after_rising) in [(59_000, Step::Think), (58_999, Step::Play(62))] {
            let mut spawn = Spawn::new(60_000, false);
            spawn.advance(60_000, 0, false, false);
            spawn.advance(60_000, 1, true, false);
            assert_eq!(spawn.advance(hp, 1, false, true), Step::Play(37));
            assert_eq!(spawn.advance(hp, 1, true, false), after_rising);
        }
    }
}
