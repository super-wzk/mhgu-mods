//! Magnet Bind buildup and landing contact for paralysis-susceptible GU monsters.
use crate::{fx, program};
use alloc::vec::Vec;
use mhgu_sdk::{
    combat::collision::hurt::{GroundHold, HurtScene, Monster, MonsterSpawn},
    geometry::Bearing,
    sync::Shared,
    weapon::style::{
        Input,
        action::{
            self,
            melee::{MonsterStrikes, RestrainedContact, StrikePower, bind_monster_strikes},
        },
        hud::{MeterLabel, MeterPanel},
        hunter,
        input::{Pad, PadButton},
        movement,
        remote::{self, MonsterExchange, MonsterState, PLAYER_COUNT},
        world_contact,
    },
};

mod camera;
mod network;
pub(super) use camera::{camera_lens, reset_camera};
use network::{Hold, Network};

pub(super) static METERS: Shared<Meters> = Shared::new(Meters::new());

pub(super) fn hosts_peer_finisher(actor: u32, member: usize, area: u8) -> bool {
    METERS.with(|meters| meters.network.hosts_caster(actor, member, area))
}

pub(super) unsafe extern "C" fn camera_yaw(actor: u32) -> i32 {
    let (group, slot) = unsafe { action::current(actor) };
    if !matches!(action::custom_id(group, slot), Some(99 | 105 | 157))
        || !unsafe { hunter::is_alive(actor) }
    {
        return -1;
    }
    // 81224300 / 81224332 ask 81678E2A for the hunter-to-monster bearing
    // after the live restraint is associated, including during the finisher.
    let area = unsafe { hunter::area(actor) };
    let target = METERS.with(|meters| {
        meters
            .binding
            .as_ref()
            .filter(|binding| binding.area == area)?
            .target
            .position()
    });
    let Some([x, _, z]) = target else { return -1 };
    let [px, _, pz] = unsafe { movement::position(actor) };
    let bearing = Bearing::between_xz((px, pz), (x, z));
    if bearing.distance.is_finite() && bearing.distance > 0. {
        i32::from(bearing.yaw.units())
    } else {
        -1
    }
}

pub(super) fn bind() -> bool {
    static OBSERVER: MonsterStrikes = MonsterStrikes {
        restrained_contact,
        begin_hunt,
        contact,
    };
    let id = crate::STYLE.id;
    bind_monster_strikes(id, &OBSERVER)
        && remote::bind_monster_state(id, receive_state)
        && remote::bind_monster_exchange(id, receive_exchange)
        && world_contact::observe_hurt(id, observe)
}

extern "C" fn restrained_contact(actor: u32, spawn: &MonsterSpawn) -> RestrainedContact {
    let Some(roster) = (unsafe { remote::Roster::current() }) else {
        return RestrainedContact::Reject;
    };
    let member = roster.peer_index(actor);
    let bound = METERS.with(|meters| {
        if actor == roster.local() {
            meters.binding.as_ref().is_some_and(|b| {
                b.target.spawn() == *spawn
                    && b.contest.phase == Phase::Finishing
                    && b.target.active()
            })
        } else {
            member.is_some_and(|i| {
                meters
                    .network
                    .caster_target(actor, i, unsafe { hunter::area(actor) })
                    == Some(*spawn)
            })
        }
    });
    if !bound {
        return RestrainedContact::Reject;
    }
    crate::magnet::marked_row(actor, member, *spawn)
        .filter(|&row| row > 0)
        .map_or(RestrainedContact::AnyPart, RestrainedContact::Row)
}

extern "C" fn begin_hunt() {
    METERS.with(Meters::clear);
    reset_camera();
}

extern "C" fn contact(actor: u32, spawn: &MonsterSpawn, power: StrikePower, affinity: f32) {
    if spawn.accepts_paralysis_in(unsafe { hunter::area(actor) }) {
        METERS.with(|meters| meters.hit(*spawn, power.adjusted(affinity)));
    }
}

extern "C" fn receive_state(state: &MonsterState) {
    if state.value & !0x00ff_7fff == 0
        && state
            .spawn
            .accepts_paralysis_in(unsafe { hunter::area(state.actor) })
    {
        METERS.with(|meters| meters.receive_state(state));
    }
}

extern "C" fn receive_exchange(actor: u32, exchange: &MonsterExchange) {
    METERS.with(|meters| {
        let hold = meters
            .meters
            .iter()
            .find(|meter| meter.spawn == exchange.spawn)
            .and_then(|meter| meter.hold);
        meters.network.receive(actor, exchange, hold);
    });
}

extern "C" fn observe(scene: &HurtScene<'_>, active: bool) {
    let frames = scene.frames();
    if frames == 0. {
        return;
    }
    let (outgoing, exchanges) = METERS.with(|meters| {
        let snapshots = meters.update(scene, frames, active);
        (snapshots, meters.network.take_outgoing())
    });
    for snapshot in outgoing {
        remote::send_monster_state(
            crate::STYLE.id,
            &snapshot.spawn,
            snapshot.serial,
            snapshot.value,
            &snapshot.detail,
        );
    }
    for exchange in exchanges {
        remote::send_monster_exchange(crate::STYLE.id, &exchange);
    }
}

// FZ 811425BC applies the percentage to the previous integer threshold.
const BIND_THRESHOLDS: [u16; 5] = {
    let mut tiers = [1800; 5];
    let mut i = 1;
    while i < tiers.len() {
        tiers[i] = (tiers[i - 1] as u32 * 190 / 100) as u16;
        i += 1;
    }
    tiers
};
const BIND_DURATION: u16 = 240;

struct Meter {
    spawn: MonsterSpawn,
    accumulated: u16,
    completed_binds: u8,
    flashes: ReadyFlashes,
    hold: Option<Hold>,
    sent: Option<(u32, u32)>,
    received: [Option<(u32, u16)>; PLAYER_COUNT],
}

struct Snapshot {
    spawn: MonsterSpawn,
    serial: u16,
    value: u32,
    detail: [u32; 2],
}

impl Meter {
    fn consume(&mut self) {
        self.accumulated = 0;
        self.completed_binds = self.completed_binds.saturating_add(1).min(4);
        self.flashes = ReadyFlashes::new(self.spawn);
    }

    fn value(&self) -> u32 {
        u32::from(self.accumulated) | u32::from(self.completed_binds) << 16
    }

    fn ready(&self) -> bool {
        self.accumulated >= BIND_THRESHOLDS[usize::from(self.completed_binds)]
    }
}

struct Landing {
    area: u8,
    center: [f32; 3],
    remaining: f32,
}

struct Binding {
    target: GroundHold,
    area: u8,
    elapsed: f32,
    contest: Contest,
    goal_announced: bool,
    serial: u16,
    effects: fx::BindCaster,
}

impl Binding {
    fn new(target: GroundHold, scene: &HurtScene<'_>, serial: u16) -> Self {
        Self {
            target,
            area: scene.area(),
            elapsed: 0.,
            contest: Contest::new(BIND_DURATION),
            goal_announced: false,
            serial,
            effects: unsafe { fx::BindCaster::confirm(scene.owner()) },
        }
    }
}

struct BoundMonster {
    spawn: MonsterSpawn,
    area: u8,
    joint: u16,
    effects: fx::BoundEffects,
}

impl BoundMonster {
    fn emit(&self, monster: &Monster<'_>, pulse: bool, aura: bool) {
        if pulse {
            self.effects.pulse(monster, self.joint);
        }
        if aura && let Some(point) = monster.joint_position(self.joint) {
            self.effects.aura(point);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Mashing,
    FinishWindow,
    Finishing,
}

pub(super) enum BindInput {
    Pending,
    Wait,
    Finish,
}

// FZ 8114345A and 81144160 use separate timers for the struggle, the
// finisher input window, and the attack itself.
struct Contest {
    phase: Phase,
    duration: u16,
    remaining: u16,
    extended: u16,
    cooldown: f32,
    fraction: f32,
    pending: bool,
    pulse: i8,
    stick_sector: Option<u16>,
}

impl Contest {
    const INITIAL_PERCENT: u16 = program::parameter(39);
    const EXTEND: u16 = program::parameter(40);
    const GOAL_PERCENT: u16 = program::parameter(41);
    const FINISH_WINDOW: u16 = program::parameter(42);
    const FINISH_TIME: u16 = program::parameter(43);

    fn monster_cap(duration: u16) -> f32 {
        2. * (f32::from(duration) + f32::from(Self::FINISH_WINDOW + Self::FINISH_TIME))
    }

    fn new(duration: u16) -> Self {
        Self {
            phase: Phase::Mashing,
            duration,
            remaining: Self::initial(duration),
            extended: 0,
            cooldown: 0.,
            fraction: 0.,
            pending: false,
            pulse: -1,
            stick_sector: None,
        }
    }

    fn initial(duration: u16) -> u16 {
        (u32::from(duration) * u32::from(Self::INITIAL_PERCENT) / 100) as u16
    }

    fn extension_budget(&self) -> u16 {
        (u32::from(self.duration) * u32::from(100 - Self::INITIAL_PERCENT) / 100) as u16
    }

    fn extension_progress(&self) -> (u32, u32) {
        (
            u32::from(self.extended) * 100,
            u32::from(self.extension_budget()) * u32::from(Self::GOAL_PERCENT),
        )
    }

    fn panel(&self) -> MeterPanel {
        static STRUGGLE: MeterLabel = MeterLabel::new(c"磁缚　转动左摇杆，连按 X 或 A");
        static READY: MeterLabel = MeterLabel::new(c"磁缚达标　等待终结窗口");
        static FINISH: MeterLabel = MeterLabel::new(c"按 ZR 使出磁缚终结");
        let (progress, goal) = self.extension_progress();
        let label = match self.phase {
            Phase::Mashing if progress >= goal => &READY,
            Phase::Mashing => &STRUGGLE,
            Phase::FinishWindow => &FINISH,
            Phase::Finishing => return MeterPanel::default(),
        };
        MeterPanel {
            label: Some(label),
            ..MeterPanel::default()
        }
    }

    fn indicator(&self) -> Option<(i32, u16)> {
        let remaining = match self.phase {
            Phase::Mashing => self.remaining,
            Phase::FinishWindow => 0,
            Phase::Finishing => return None,
        };
        let (progress, goal) = self.extension_progress();
        let travel = 1. - f32::from(remaining) / f32::from(Self::initial(self.duration).max(1));
        let first = if progress >= goal { 520 } else { 509 };
        let frame = if self.pulse < 0 {
            0
        } else {
            (10 - self.pulse) as u16
        };
        Some(((travel * 122.) as i32, first + frame))
    }

    fn record(&mut self, input: &Input, pad: &Pad) -> BindInput {
        match self.phase {
            Phase::Mashing => {
                // GU 27E608 treats strengths above 49 as directional input.
                // A transition between cardinal sectors stands in for an FZ
                // left-stick direction edge during a circle.
                let sector = (input.stick_strength > 49)
                    .then(|| input.stick_angle.wrapping_add(0x2000) >> 14);
                self.pending |= sector.is_some() && sector != self.stick_sector
                    || pad.pressed(PadButton::X)
                    || pad.pressed(PadButton::A);
                self.stick_sector = sector;
                BindInput::Wait
            }
            Phase::FinishWindow if pad.pressed(PadButton::Zr) => BindInput::Finish,
            _ => BindInput::Wait,
        }
    }

    fn advance(&mut self, frames: f32) -> bool {
        self.fraction += frames;
        while self.fraction >= 2. {
            self.fraction -= 2.;
            if self.phase == Phase::Mashing {
                let press = core::mem::take(&mut self.pending);
                let extension = self.extension_budget();
                let spacing = (f32::from(Self::initial(self.duration)) * f32::from(Self::EXTEND)
                    / f32::from(extension))
                .max(1.);
                if press && self.cooldown < 1. {
                    self.pulse = 10;
                    self.extended = self.extended.saturating_add(Self::EXTEND);
                    self.remaining = self
                        .remaining
                        .saturating_add(Self::EXTEND)
                        .min(self.duration);
                    self.cooldown += f32::from(Self::EXTEND) + spacing;
                }
                self.cooldown = (self.cooldown - 1.).max(spacing - (spacing as u16) as f32 - 1.);
            }
            self.pulse = (self.pulse - 1).max(-1);
            self.remaining = self.remaining.saturating_sub(1);
            if self.remaining != 0 {
                continue;
            }
            let (progress, goal) = self.extension_progress();
            if self.phase != Phase::Mashing || progress < goal {
                return false;
            }
            self.phase = Phase::FinishWindow;
            self.remaining = Self::FINISH_WINDOW - 1; // 81144160 also runs on the transition tick.
        }
        true
    }

    fn begin_finisher(&mut self) {
        if self.phase == Phase::FinishWindow {
            self.phase = Phase::Finishing;
            self.remaining = Self::FINISH_TIME;
        }
    }
}

pub(super) struct Meters {
    meters: Vec<Meter>,
    landing: Option<Landing>,
    binding: Option<Binding>,
    network: Network,
    effects: Vec<BoundMonster>,
    serial: u16,
    scene_time: f32,
}

impl Meters {
    pub const fn new() -> Self {
        Self {
            meters: Vec::new(),
            landing: None,
            binding: None,
            network: Network::new(),
            effects: Vec::new(),
            serial: 0,
            scene_time: 0.,
        }
    }

    pub fn end_local(&mut self) {
        if let Some(binding) = &self.binding
            && binding.target.locally_owned()
        {
            self.effects
                .retain(|effect| effect.spawn != binding.target.spawn());
        }
        self.release();
        self.network.cancel_pending();
        self.landing = None;
    }

    fn clear(&mut self) {
        self.end_local();
        self.meters.clear();
        self.effects.clear();
        self.network.clear();
        self.serial = 0;
        self.scene_time = 0.;
    }

    pub fn begin_landing(&mut self, area: u8, center: [f32; 3], late: f32) {
        self.network.cancel_pending();
        self.landing = Some(Landing {
            area,
            center,
            remaining: program::BIND_CONTACT.0 - late,
        });
    }

    pub fn input(&mut self, input: &Input, pad: &Pad, frames: f32) -> Option<BindInput> {
        if self.network.pending() {
            return Some(BindInput::Pending);
        }
        self.binding
            .as_mut()
            .filter(|binding| binding.target.active())
            .map(|binding| {
                if frames > 0. {
                    binding.contest.record(input, pad)
                } else {
                    BindInput::Wait
                }
            })
    }

    pub fn begin_finisher(&mut self) {
        if let Some(binding) = &mut self.binding {
            binding.contest.begin_finisher();
        }
    }

    pub fn indicator(&self) -> Option<(i32, u16)> {
        self.binding.as_ref()?.contest.indicator()
    }

    pub fn panel(&self) -> MeterPanel {
        self.binding
            .as_ref()
            .map_or_else(MeterPanel::default, |binding| binding.contest.panel())
    }

    pub fn finisher_target(&self) -> Option<[f32; 3]> {
        let binding = self
            .binding
            .as_ref()
            .filter(|binding| binding.contest.phase == Phase::Finishing)?;
        binding.target.position()
    }

    fn release(&mut self) {
        let Some(binding) = self.binding.take() else {
            return;
        };
        let spawn = binding.target.spawn();
        if binding.target.locally_owned() {
            if let Some(meter) = self.meters.iter_mut().find(|meter| meter.spawn == spawn) {
                let transferred = meter.hold.is_some_and(|hold| {
                    hold.serial == binding.serial
                        && Some(hold.caster) == binding.target.network_controller()
                        && hold.elapsed > 60.
                });
                // FZ 811425BC consumes readiness after more than 30 FZ updates.
                if binding.elapsed > 60. || transferred {
                    meter.consume();
                }
            }
            binding.target.release();
        } else if let Some(member) = binding.target.network_controller() {
            self.network
                .end_binding(spawn, binding.serial, member, binding.area);
        }
    }

    fn meter(&mut self, spawn: MonsterSpawn) -> &mut Meter {
        let create = || Meter {
            spawn,
            accumulated: 0,
            completed_binds: 0,
            flashes: ReadyFlashes::new(spawn),
            hold: None,
            sent: None,
            received: [None; PLAYER_COUNT],
        };
        let existing = self
            .meters
            .iter()
            .position(|meter| (meter.spawn.kind, meter.spawn.set_id) == (spawn.kind, spawn.set_id));
        if let Some(index) = existing {
            let existing = &mut self.meters[index];
            if existing.spawn != spawn {
                *existing = create();
            }
            existing
        } else {
            self.meters.push(create());
            self.meters.last_mut().unwrap()
        }
    }

    fn hit(&mut self, spawn: MonsterSpawn, power: u16) {
        if self.network.hold(spawn).is_some()
            || self
                .binding
                .as_ref()
                .is_some_and(|binding| binding.target.spawn() == spawn)
        {
            return;
        }
        // FZ 81142DF2 takes the resolved motion power. 81142D76 caps
        // the accumulated value at the maximum positive signed halfword.
        let gain = u32::from(power) * u32::from(program::parameter(38)) / 100;
        if gain == 0 {
            return;
        }
        let meter = self.meter(spawn);
        meter.accumulated = (u32::from(meter.accumulated) + gain).min(i16::MAX as u32) as u16;
    }

    fn receive_state(&mut self, state: &MonsterState) {
        let meter = self.meter(state.spawn);
        let Some(previous) = meter.received.get_mut(usize::from(state.controller)) else {
            return;
        };
        if previous.is_some_and(|(actor, serial)| {
            actor == state.actor && !(1..0x8000).contains(&state.serial.wrapping_sub(serial))
        }) {
            return;
        }
        *previous = Some((state.actor, state.serial));
        let accumulated = state.value as u16;
        if accumulated < meter.accumulated {
            meter.flashes = ReadyFlashes::new(state.spawn);
        }
        meter.accumulated = accumulated;
        meter.completed_binds = ((state.value >> 16) as u8).min(4);
        meter.hold = Hold::from_wire(state.detail);
        meter.sent = None;
    }

    fn update(&mut self, scene: &HurtScene<'_>, frames: f32, active: bool) -> Vec<Snapshot> {
        // 81142966 and 81142A7C use the global 16-bit FZ scene tick.
        // At GU's simulation rate each source tick covers two frames.
        const SCENE_WRAP: f32 = 2. * 65_536.;
        let next = self.scene_time + frames;
        let crossed = |period| {
            next >= SCENE_WRAP || (self.scene_time / period) as u32 != (next / period) as u32
        };
        let (pulse, aura) = (crossed(32.), crossed(60.));
        self.scene_time = next % SCENE_WRAP;
        let area = scene.area();
        if !active && (self.landing.is_some() || self.binding.is_some() || self.network.pending()) {
            self.end_local();
        }
        let state = if self.landing.is_some() || self.binding.is_some() || self.network.pending() {
            let (group, slot) = unsafe { action::current(scene.owner()) };
            action::custom_id(group, slot)
        } else {
            None
        };
        if let Some(binding) = &mut self.binding {
            let current = binding.area == area && matches!(state, Some(98 | 99 | 105 | 157));
            if current
                && let Some(monster) = binding.target.resolve(scene)
                && monster.ground_held()
                && (state == Some(98) || binding.contest.advance(frames))
            {
                let previous = binding.elapsed;
                binding.elapsed += frames;
                let joint = monster.hurt_joints().next().unwrap_or(0);
                let (progress, goal) = binding.contest.extension_progress();
                if !binding.goal_announced && progress >= goal {
                    binding.goal_announced = true;
                    fx::contest_goal(scene.owner(), &monster, joint);
                }
                if let Some(point) = monster
                    .joint_position(joint)
                    .or_else(|| binding.target.position())
                {
                    unsafe {
                        binding.effects.advance(
                            scene.owner(),
                            previous,
                            binding.elapsed,
                            state == Some(99),
                            point,
                        )
                    };
                }
            } else {
                if current && unsafe { hunter::is_alive(scene.owner()) } {
                    unsafe {
                        binding.effects.release(scene.owner());
                        let frame = action::MotionFrame::sample(scene.owner());
                        if !matches!(state, Some(105 | 157))
                            || frame.blend > 0.
                            || frame.frame < 184.
                        {
                            crate::sound::bind_caster(scene.owner(), 24);
                        }
                    };
                }
                self.release();
            }
        }
        let mut landing = self.landing.take().filter(|landing| {
            matches!(state, Some(98 | 99)) && landing.area == area && landing.remaining > 0.
        });
        let center = landing.as_ref().map(|window| window.center);
        if let Some(window) = &mut landing {
            window.remaining -= frames;
        }
        self.landing = landing.filter(|landing| landing.remaining > 0.);
        let mut outgoing = Vec::new();
        let in_quest = scene.has_active_quest();
        self.update_holds(scene, frames, in_quest);
        self.network
            .update_controller(scene, frames, in_quest, &mut self.meters);
        if let Some((target, ticket)) =
            self.network
                .confirm(scene, frames, matches!(state, Some(98 | 99)))
        {
            if in_quest {
                self.binding = Some(Binding::new(target, scene, ticket.serial()));
            } else {
                self.network.end(ticket);
            }
        }
        let monsters = scene.snapshot_monsters();
        let meter_candidates = (!self.meters.is_empty()).then(|| monsters.iter().flatten());
        for monster in meter_candidates.into_iter().flatten() {
            let Some(meter) = self
                .meters
                .iter_mut()
                .find(|meter| meter.spawn == monster.spawn())
            else {
                continue;
            };
            if !monster.accepts_paralysis() {
                continue;
            }
            let local = monster.locally_owned();
            if !monster.ground_held() && in_quest && meter.ready() {
                // FZ 816FAA64 tests the contact before assigning a single target.
                // 81142BE0 sets the separate monster-side cap.
                if let Some(center) = center
                    && self.binding.is_none()
                    && !self.network.pending()
                    && monster.overlaps(center, program::BIND_CONTACT.1)
                {
                    if local {
                        if let Some(target) =
                            monster.try_ground_hold(Contest::monster_cap(BIND_DURATION))
                        {
                            self.landing = None;
                            self.binding =
                                Some(Binding::new(target, scene, self.network.next_serial()));
                        }
                    } else if let Some(member) = monster.network_controller()
                        && monster.can_ground_hold()
                    {
                        self.network.request(monster.spawn(), member, area, center);
                        self.landing = None;
                    }
                }
                if !monster.ground_held() {
                    meter.flashes.update(monster, frames);
                }
            }
            if local && let Some(member) = monster.network_controller() {
                let binding = self
                    .binding
                    .as_ref()
                    .filter(|binding| binding.target.spawn() == meter.spawn);
                meter.hold = binding
                    .map(|binding| {
                        let elapsed = meter
                            .hold
                            .filter(|hold| hold.caster == member && hold.serial == binding.serial)
                            .map_or(binding.elapsed, |hold| binding.elapsed.max(hold.elapsed));
                        Hold {
                            caster: member,
                            serial: binding.serial,
                            elapsed,
                        }
                    })
                    .or_else(|| self.network.hold(meter.spawn));
                let detail = meter.hold.map_or([0; 2], Hold::wire);
                let value = meter.value();
                // Publish immediately when the hold starts, ends, or crosses
                // the resistance threshold. The heartbeat carries its age.
                let key = (
                    value,
                    detail[0] | u32::from(meter.hold.is_some_and(|hold| hold.elapsed > 60.)) << 19,
                );
                if meter.sent != Some(key) || aura {
                    self.serial = self.serial.wrapping_add(1);
                    outgoing.push(Snapshot {
                        spawn: meter.spawn,
                        serial: self.serial,
                        value,
                        detail,
                    });
                    meter.sent = Some(key);
                }
            } else {
                meter.sent = None;
            }
        }
        self.update_effects(&monsters, area, pulse, aura);
        outgoing
    }

    fn update_holds(&mut self, scene: &HurtScene<'_>, frames: f32, in_quest: bool) {
        for meter in &mut self.meters {
            let Some(hold) = &mut meter.hold else {
                continue;
            };
            let Some(monster) = scene.find_spawn(meter.spawn) else {
                continue;
            };
            let Some(target) = monster.observe_ground_hold() else {
                continue;
            };
            if !monster.locally_owned() {
                hold.elapsed = (hold.elapsed + frames).min(4095.);
            } else if let Some(member) = monster.network_controller()
                && member != hold.caster
                && in_quest
            {
                self.network.adopt(*hold, target, scene.area());
            }
        }
    }

    fn update_effects(
        &mut self,
        monsters: &[Option<Monster<'_>>],
        area: u8,
        pulse: bool,
        aura: bool,
    ) {
        let monsters = || monsters.iter().flatten();
        self.effects.retain(|saved| {
            if saved.area != area {
                return false;
            }
            let Some(monster) = monsters().find(|monster| monster.spawn() == saved.spawn) else {
                return false;
            };
            if monster.ground_held() {
                return true;
            }
            saved.effects.release(monster, saved.joint);
            false
        });
        for monster in monsters().filter(|monster| monster.ground_held()) {
            let spawn = monster.spawn();
            let index = self.effects.iter().position(|saved| saved.spawn == spawn);
            let effect = if let Some(index) = index {
                &self.effects[index]
            } else {
                let joint = monster.hurt_joints().next().unwrap_or(0);
                self.effects.push(BoundMonster {
                    spawn,
                    area,
                    joint,
                    effects: fx::BoundEffects::new(),
                });
                self.effects.last().unwrap()
            };
            effect.emit(monster, pulse, aura);
        }
    }
}

struct ReadyFlashes {
    random: u32,
    joint: u16,
    next: usize,
    pattern: usize,
    until_pulse: f32,
}

impl ReadyFlashes {
    fn new(spawn: MonsterSpawn) -> Self {
        Self {
            random: 1
                + ((u32::from(spawn.kind) << 16) ^ u32::from(spawn.set_id))
                + u32::from(spawn.revision),
            joint: 0,
            next: 0,
            pattern: 0,
            until_pulse: 0.,
        }
    }

    fn random(&mut self) -> usize {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        self.random as usize
    }

    fn update(&mut self, monster: &Monster<'_>, frames: f32) {
        self.until_pulse -= frames;
        if self.until_pulse > 0. {
            return;
        }
        if self.next == 0 {
            // GU has its own skeletons and hit groups. Use only active hurt
            // joints for the center of each cluster, as 81142EEE does in FZ.
            let joints: Vec<_> = monster.hurt_joints().collect();
            if joints.is_empty() {
                self.until_pulse = 2.;
                return;
            }
            self.joint = joints[self.random() % joints.len()];
        }
        let offset = self.random() % 3;
        let joint = (self.joint + offset as u16).saturating_sub(1);
        if let Some(position) = monster
            .joint_position(joint)
            .or_else(|| monster.joint_position(self.joint))
        {
            fx::bind_ready(position);
        }
        // 81142EEE rotates three patterns. Each has three pulses at 0, d
        // and 2d, followed by another d and a short gap before the next.
        let (spacing, gap) = [(11., 4.), (20., 6.), (10., 4.)][self.pattern];
        self.until_pulse = 2. * spacing;
        self.next += 1;
        if self.next == 3 {
            self.until_pulse += 2. * gap;
            self.next = 0;
            self.pattern = (self.pattern + 1) % 3;
        }
    }
}
