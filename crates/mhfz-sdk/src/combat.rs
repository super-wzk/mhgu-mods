//! Frontier attack, guard and evade timing on GU's motion clock.

/// Preserve SDT byte 29, which FZ 816F9840 / 81820D7A count in 30 Hz ticks.
pub const fn attacker_hitstop(source_ticks: u8) -> u8 {
    mhgu_sdk::weapon::style::action::hitstop::frontier(source_ticks)
}

/// Convert SDT delay and duration to GU contact clock units. 8183F4DE
/// truncates them to FZ ticks. 817006A6 activates one tick after delay
/// reaches zero and spends the first duration tick there; GU 16919C
/// activates on reaching zero and first spends duration on the next tick.
pub fn contact_window(delay: u16, duration: u16) -> (f32, f32) {
    let delay = f32::from(delay & !1);
    let duration = f32::from(duration & !1);
    let activation_tick = if delay > 0. { 2. } else { 0. };
    (
        delay + activation_tick,
        (duration - activation_tick).max(0.),
    )
}

/// FZ 816FD8A2 consumes the first matching contact of either blade, even
/// when the effective hitzone is soft enough to keep swinging.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub extern "C" fn deflection(factor: f32) -> mhgu_sdk::weapon::style::rules::Deflection {
    use mhgu_sdk::weapon::style::rules::Deflection;
    if factor < 0.23 {
        Deflection::Recoil
    } else {
        Deflection::CloseWindow
    }
}

/// Extra motion frames for Frontier guard windows. GU 2F4A2C applies the
/// stronger Guard skill first, including the two compound skills.
///
/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn guard_bonus_frames(actor: u32) -> u16 {
    use mhgu_sdk::weapon::style::hunter::has_skill;
    if [168, 279, 297]
        .into_iter()
        .any(|id| unsafe { has_skill(actor, id) })
    {
        4
    } else if unsafe { has_skill(actor, 169) } {
        2
    } else {
        0
    }
}

/// Extra GU clock frames for a Frontier action that benefits from Evasion.
///
/// # Safety
/// The actor must be a live, equipped GU hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn evade_bonus_frames(actor: u32) -> f32 {
    match unsafe { mhgu_sdk::weapon::style::hunter::evasion_level(actor) } {
        2.. => 10.,
        1 => 6.,
        _ => 0.,
    }
}

/// Frontier integrates ordinary steps once per logic tick. Weapon-specific
/// windows start after the motion blend and may scale by the playhead advance.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvadeClock {
    Common,
    WeaponTick,
    WeaponMotion,
}

/// Signed local-axis travel from FZ 817CC02A, 81811B0C or 81811C5E.
pub struct EvadeDistance {
    side: bool,
    distance: f32,
    start: f32,
    end: f32,
}

impl EvadeDistance {
    pub fn side(distance: f32, start: f32, end: f32) -> Self {
        Self {
            side: true,
            distance,
            start,
            end,
        }
    }

    pub fn forward(distance: f32, start: f32, end: f32) -> Self {
        Self {
            side: false,
            distance,
            start,
            end,
        }
    }

    fn portion(&self, frame: f32, next: f32, elapsed: f32) -> f32 {
        let overlap = next.min(self.end) - frame.max(self.start);
        if next <= frame || overlap <= 0. || elapsed <= 0. {
            return 0.;
        }
        self.distance / (self.end - self.start) * elapsed * (overlap / (next - frame))
    }

    /// Apply once per local private-action update. 81811C5E scales by the
    /// playhead advance; the other two helpers run once per 30 Hz logic tick.
    /// # Safety
    /// The actor must be the live local quest hunter.
    #[cfg(all(target_arch = "arm", target_endian = "little"))]
    pub unsafe fn apply(
        &self,
        actor: u32,
        sample: mhgu_sdk::weapon::style::action::MotionFrame,
        clock: EvadeClock,
    ) {
        use mhgu_sdk::weapon::style::{hunter, movement};
        if clock != EvadeClock::Common && sample.blend > 0. {
            return;
        }
        if sample.frame >= self.end || sample.next <= self.start || sample.next <= sample.frame {
            return;
        }
        if ![166, 267, 285, 318]
            .into_iter()
            .any(|id| unsafe { hunter::has_skill(actor, id) })
        {
            return;
        }
        let elapsed = if clock == EvadeClock::WeaponMotion {
            sample.next - sample.frame
        } else {
            (unsafe { movement::motion_frames(actor) }) * 0.5
        };
        let delta = self.portion(sample.frame, sample.next, elapsed);
        let (side, forward) = if self.side { (delta, 0.) } else { (0., delta) };
        unsafe { movement::displace_horizontal(actor, side, forward) };
    }
}

/// GU launch directions eligible for the FZ drawn recovery (817E9CB4 / 817EAA30).
///
/// # Safety
/// The actor must be a live GU quest hunter on the game thread.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn knockback_recovery(
    actor: u32,
    current: (u16, u16),
    drawn: bool,
    input: &mhgu_sdk::weapon::style::Input,
    states: [u8; 2],
) -> Option<u8> {
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
    use mhgu_sdk::weapon::style::movement;

    // Only the local quest controller reads inputs. Phase 0 starts a new launch.
    static PENDING: AtomicU32 = AtomicU32::new(0);
    if !drawn
        || !matches!(current, (64, 6 | 7))
        || unsafe { ((actor + 5078) as *const i16).read() } <= 0
    {
        PENDING.store(0, Relaxed);
        return None;
    }
    unsafe {
        let phase = ((actor + 5) as *const u8).read();
        let pressed = input.command(3);
        let held = ((actor + 5108) as *const u16).read() & 0x80 != 0;
        let address = forge_sys::mem::forge_mem_getMainTextAddr() + 0x53a058;
        let motion: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(address as usize);
        match phase {
            1 => {
                // FZ 81213142 accepts recovery below floor + 150. GU 2BB164
                // updates the floor while descending, then attaches at landing.
                // Keep a near-ground press until that attachment has completed.
                if pressed
                    && ((actor + 4116) as *const f32).read() <= 0.
                    && ((actor + 9512) as *const f32).read() <= 0.
                    && ((actor + 68) as *const f32).read()
                        < ((actor + 4336) as *const f32).read() + 150.
                    && matches!(motion(actor), 1013 | 1014 | 1019 | 1020)
                {
                    PENDING.store(actor, Relaxed);
                }
                None
            }
            2 if ((actor + 11988) as *const u8).read() != 0 && motion(actor) == 1015 => {
                let queued = PENDING.swap(0, Relaxed) == actor;
                ((pressed || held || queued) && movement::has_evade_stamina(actor, 150.))
                    .then_some(states[usize::from(current.1 - 6)])
            }
            _ => {
                PENDING.store(0, Relaxed);
                None
            }
        }
    }
}

#[repr(u32)]
pub enum DrawnEvade {
    Roll = 0,
    DemonStep = 6,
}

/// Apply GU's Bladescale Hone and weapon-specific evade progress once on entry.
///
/// # Safety
/// Call for an accepted local drawn evade that bypasses GU's evade action.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn bladescale_evade(actor: u32, kind: DrawnEvade) {
    let address = unsafe { forge_sys::mem::forge_mem_getMainTextAddr() } + 0x2df9d0;
    let apply: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(address as usize) };
    unsafe { apply(actor, kind as u32) };
}

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
