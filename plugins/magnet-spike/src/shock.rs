//! Frontier's magnetic impact requests run independently of the action.
use crate::{STYLE, fx::Route, magnet, motion, peer, program};
use alloc::vec::Vec;
use mhgu_sdk::{
    combat::collision::{CachedHitShape, Contact, ContactPool, scene_frames},
    sync::Shared,
    weapon::style::{
        action::melee::{self, Strike, StrikePower},
        hunter, particle,
        remote::{self, Roster},
        world_contact,
    },
};

#[derive(Clone, Copy)]
enum Event {
    Opening(Route),
    Flash(Route),
    LocalFlash(Route),
    Hit(usize, Option<Route>),
}

// FZ 8102CDDC request 15, in GU scene frames. Take the root and yaw at each
// event's birth. The resulting capsules and flashes stay at their world points.
const HIT_COUNT: usize = 4;
type TimedEvent = (f32, [f32; 3], Event);
const ENHANCED: &[TimedEvent] = &[
    (0., [-90., 0., 0.], Event::Opening(Route::ShockOpening)),
    (0., [-120., 0., 80.], Event::Flash(Route::ShockOpening)),
    (2., [-130., 0., 130.], Event::Flash(Route::ShockOpening)),
    (2., [-30., 140., 250.], Event::LocalFlash(Route::ShockLocal)),
    (
        6.,
        [-120., 0., 170.],
        Event::Hit(0, Some(Route::ShockFirst)),
    ),
    (
        10.,
        [-90., 0., 250.],
        Event::Hit(1, Some(Route::ShockFirst)),
    ),
    (12., [-30., 0., 400.], Event::Flash(Route::ShockMiddle)),
    (14., [-30., 0., 400.], Event::Hit(2, None)),
    (16., [40., 0., 550.], Event::Flash(Route::ShockFar)),
    (20., [40., 0., 550.], Event::Hit(3, None)),
];
// FZ 8102CDDC request 16 uses the same scene clock with its own visual offsets.
const ORDINARY: &[TimedEvent] = &[
    (0., [-150., 0., 0.], Event::Opening(Route::ImpactOpening)),
    (2., [-180., 0., 70.], Event::Flash(Route::ImpactOpening)),
    (
        2.,
        [-30., 140., 250.],
        Event::LocalFlash(Route::ImpactLocal),
    ),
    (4., [-170., 0., 140.], Event::Flash(Route::ImpactOpening)),
    (6., [-110., 0., 270.], Event::Flash(Route::ImpactMiddle)),
    (10., [-20., 0., 450.], Event::Flash(Route::ImpactFar)),
];
static SHAPE: CachedHitShape = CachedHitShape::new(c"player\\hit\\frontier_ms\\shock");
// GU retains intrusive contact links. Keep the backing storage alive even
// after a pulse expires, and reuse only sequences with no active contacts.
static SEQUENCES: Shared<Vec<Sequence>> = Shared::new(Vec::new());

struct Sequence {
    owner: u32,
    area: u8,
    age: f32,
    next: usize,
    timeline: &'static [TimedEvent],
    pulses: [Pulse; HIT_COUNT],
    contacts: Option<ContactPool>,
    sampled: f32,
}

#[derive(Clone, Copy)]
struct Pulse {
    point: [f32; 3],
    power: StrikePower,
    direction: [f32; 3],
}

impl Sequence {
    fn hits(&self) -> impl Iterator<Item = (Contact<'_>, &Pulse)> {
        self.contacts
            .iter()
            .flat_map(ContactPool::iter)
            .zip(&self.pulses)
    }

    fn stop(&mut self) {
        self.owner = 0;
        for (contact, _) in self.hits() {
            contact.disable();
        }
    }

    unsafe fn advance(&mut self, manager: u32, frames: f32, local: bool) {
        while let Some(&(at, offset, event)) = self
            .timeline
            .get(self.next)
            .filter(|entry| entry.0 <= self.age)
        {
            self.next += 1;
            let (point, yaw) = unsafe { particle::world_offset(self.owner, offset) };
            unsafe {
                match event {
                    Event::Opening(route) => {
                        crate::fx::shock(route, point, yaw);
                        crate::sound::shock(self.owner, point);
                    }
                    Event::Flash(route) => crate::fx::shock(route, point, yaw),
                    Event::LocalFlash(route) if local => crate::fx::shock(route, point, yaw),
                    Event::LocalFlash(_) => {}
                    Event::Hit(index, flash) => {
                        if let Some(route) = flash {
                            crate::fx::shock(route, point, yaw);
                        }
                        self.spawn(
                            index,
                            point,
                            self.age - at,
                            local,
                            program::SHOCK_STRIKE,
                            [0.; 3],
                        );
                    }
                }
            }
        }
        self.sampled = self.age;
        let mut active = false;
        for (contact, _) in self.hits().filter(|(contact, _)| contact.enabled()) {
            unsafe { contact.enqueue(manager) };
            active = true;
        }
        self.age += frames;
        if self.next == self.timeline.len() && !active {
            self.owner = 0;
        }
    }

    unsafe fn spawn(
        &mut self,
        index: usize,
        point: [f32; 3],
        late: f32,
        local: bool,
        mut strike: Strike,
        direction: [f32; 3],
    ) {
        strike.duration -= (late - strike.delay).max(0.);
        strike.delay = (strike.delay - late).max(0.);
        if strike.duration <= 0. {
            return;
        }
        let Some(shape) = (unsafe { SHAPE.load() }) else {
            return;
        };
        let (mode, boosts) = if local {
            (
                motion::local_pose(self.owner).unwrap_or_default().mode,
                magnet::boosts(self.owner),
            )
        } else {
            let pose = unsafe { peer::pose(self.owner) }.unwrap_or_default();
            (pose.mode, pose.boosts)
        };
        strike.power.bonus = program::power_bonus(boosts, mode);
        self.pulses[index] = Pulse {
            point,
            power: strike.power,
            direction,
        };
        let contacts = self
            .contacts
            .get_or_insert_with(|| unsafe { ContactPool::new(HIT_COUNT) });
        let contact = contacts.iter().nth(index).unwrap();
        unsafe { contact.prepare_world_strike(self.owner, &strike, &shape) };
    }
}

pub(super) unsafe fn preload() {
    unsafe { SHAPE.preload() };
}

pub(super) unsafe fn start(actor: u32, enhanced: bool) {
    unsafe {
        begin(actor, |sequence| {
            sequence.timeline = if enhanced { ENHANCED } else { ORDINARY }
        })
    };
}

pub(super) unsafe fn detached(
    actor: u32,
    row: u8,
    point: [f32; 3],
    velocity: [f32; 3],
    late: f32,
    local: bool,
) {
    unsafe {
        begin(actor, |sequence| {
            sequence.age = late;
            sequence.spawn(
                0,
                point,
                late,
                local,
                program::detached_strike(row),
                velocity,
            );
        })
    };
}

unsafe fn begin(actor: u32, init: impl FnOnce(&mut Sequence)) {
    let area = unsafe { hunter::area(actor) };
    SEQUENCES.with(|sequences| {
        let index = sequences.iter().position(|sequence| sequence.owner == 0);
        let sequence = if let Some(index) = index {
            &mut sequences[index]
        } else {
            sequences.push(Sequence {
                owner: 0,
                area,
                age: 0.,
                next: 0,
                timeline: &[],
                pulses: [Pulse {
                    point: [0.; 3],
                    power: program::SHOCK_STRIKE.power,
                    direction: [0.; 3],
                }; HIT_COUNT],
                contacts: None,
                sampled: 0.,
            });
            sequences.last_mut().unwrap()
        };
        sequence.owner = actor;
        sequence.area = area;
        sequence.age = 0.;
        sequence.next = 0;
        sequence.sampled = 0.;
        sequence.timeline = &[];
        init(sequence);
    });
}

pub(super) fn reset(actor: u32) {
    if actor != 0 {
        crate::native_media::reset(Some(actor));
        SEQUENCES.with(|sequences| {
            for sequence in sequences
                .iter_mut()
                .filter(|sequence| sequence.owner == actor)
            {
                sequence.stop();
            }
        });
    }
}

unsafe extern "C" fn pass(manager: u32) {
    let local = unsafe { hunter::local() }.filter(|&actor| unsafe { hunter::is_quest(actor) });
    let area = local.map(|actor| unsafe { hunter::area(actor) });
    let roster = unsafe { Roster::current() };
    let frames = scene_frames();
    unsafe { crate::native_media::advance(local, area, roster.as_ref(), frames) };
    SEQUENCES.with(|sequences| {
        for sequence in sequences.iter_mut().filter(|sequence| sequence.owner != 0) {
            let owner = sequence.owner;
            let is_local = local == Some(owner);
            let live = area == Some(sequence.area)
                && (is_local
                    || roster
                        .as_ref()
                        .is_some_and(|roster| roster.peer_index(owner).is_some())
                        && remote::is_position_event_peer(STYLE.id, owner));
            if !live || !unsafe { hunter::area(owner) == sequence.area && hunter::is_alive(owner) }
            {
                sequence.stop();
            } else if frames > 0. {
                unsafe { sequence.advance(manager, frames, is_local) };
            }
        }
    });
}

unsafe extern "C" fn expand(owner: u32, hit: u32) -> u32 {
    SEQUENCES.with(|sequences| {
        let (radius, height) = program::SHOCK_CAPSULE;
        for sequence in sequences.iter().filter(|sequence| sequence.owner == owner) {
            for (
                contact,
                &Pulse {
                    point, direction, ..
                },
            ) in sequence.hits()
            {
                if contact.as_raw() == hit && contact.enabled() {
                    let ticks = libm::floorf(sequence.sampled * 0.5);
                    let [x, y, z] = core::array::from_fn(|i| point[i] + direction[i] * ticks);
                    return u32::from(unsafe {
                        contact.submit_world_capsule([[x, y + height, z], [x, y, z]], radius)
                    });
                }
            }
        }
        0
    })
}

extern "C" fn strike_power(owner: u32, hit: u32, power: &mut StrikePower) -> bool {
    SEQUENCES.with(|sequences| {
        let Some((_, pulse)) = sequences
            .iter()
            .filter(|sequence| sequence.owner == owner)
            .flat_map(Sequence::hits)
            .find(|(contact, _)| contact.as_raw() == hit && contact.enabled())
        else {
            return false;
        };
        *power = pulse.power;
        true
    })
}

pub(super) fn bind() -> bool {
    (unsafe { world_contact::bind(STYLE.id, expand) && world_contact::bind_pass(STYLE.id, pass) })
        && melee::bind_world_strike_power(STYLE.id, strike_power)
}
