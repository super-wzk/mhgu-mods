use super::{BIND_DURATION, Contest, Meter, program};
use alloc::vec::Vec;
use mhgu_sdk::{
    combat::collision::hurt::{GroundHold, HurtScene, MonsterSpawn},
    weapon::style::{
        action, hunter, movement,
        remote::{self, MonsterCommand, MonsterExchange, PLAYER_COUNT, Roster},
    },
};

// FZ 8116648E can wait for a nonlocal monster association after the
// landing contact. Bound both sides of the GU handshake on the scene clock.
const WAIT: f32 = 120.;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Ticket {
    spawn: MonsterSpawn,
    serial: u16,
    member: u8,
    area: u8,
}

impl Ticket {
    pub fn serial(self) -> u16 {
        self.serial
    }

    fn packet(self, command: MonsterCommand, center: [f32; 3]) -> MonsterExchange {
        MonsterExchange {
            spawn: self.spawn,
            serial: self.serial,
            member: self.member,
            command,
            area: self.area,
            center,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Hold {
    pub caster: u8,
    pub serial: u16,
    pub elapsed: f32,
}

impl Hold {
    const PRESENT: u32 = 1 << 18;

    pub fn wire(self) -> [u32; 2] {
        [
            Self::PRESENT | u32::from(self.caster) << 16 | u32::from(self.serial),
            self.elapsed.to_bits(),
        ]
    }

    pub fn from_wire([binding, age]: [u32; 2]) -> Option<Self> {
        let elapsed = f32::from_bits(age);
        if binding >> 18 != 1 || !(0. ..=4095.).contains(&elapsed) {
            return None;
        }
        Some(Self {
            caster: (binding >> 16 & 3) as u8,
            serial: binding as u16,
            elapsed,
        })
    }
}

struct Pending {
    ticket: Ticket,
    remaining: f32,
    confirmed: bool,
}

struct Request {
    ticket: Ticket,
    center: [f32; 3],
    remaining: f32,
}

struct Hosted {
    ticket: Ticket,
    target: GroundHold,
    elapsed: f32,
    ended: bool,
}

struct Peer {
    actor: u32,
    last_serial: Option<u16>,
    last_end: Option<Ticket>,
    request: Option<Request>,
    held: Option<Hosted>,
}

impl Peer {
    const fn new() -> Self {
        Self {
            actor: 0,
            last_serial: None,
            last_end: None,
            request: None,
            held: None,
        }
    }
}

pub(super) struct Network {
    next_serial: u16,
    pending: Option<Pending>,
    peers: [Peer; PLAYER_COUNT],
    outgoing: Vec<MonsterExchange>,
}

impl Network {
    pub const fn new() -> Self {
        Self {
            next_serial: 0,
            pending: None,
            peers: [const { Peer::new() }; PLAYER_COUNT],
            outgoing: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        for peer in &mut self.peers {
            if let Some(held) = peer.held.take() {
                held.target.release();
            }
        }
        *self = Self::new();
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn next_serial(&mut self) -> u16 {
        self.next_serial = self.next_serial.wrapping_add(1);
        self.next_serial
    }

    pub fn request(&mut self, spawn: MonsterSpawn, member: u8, area: u8, center: [f32; 3]) {
        let ticket = Ticket {
            spawn,
            serial: self.next_serial(),
            member,
            area,
        };
        self.pending = Some(Pending {
            ticket,
            remaining: WAIT,
            confirmed: false,
        });
        self.outgoing
            .push(ticket.packet(MonsterCommand::Request, center));
    }

    pub fn end(&mut self, ticket: Ticket) {
        self.outgoing
            .push(ticket.packet(MonsterCommand::End, [0.; 3]));
    }

    pub fn end_binding(&mut self, spawn: MonsterSpawn, serial: u16, member: u8, area: u8) {
        self.end(Ticket {
            spawn,
            serial,
            member,
            area,
        });
    }

    pub fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.end(pending.ticket);
        }
    }

    pub fn receive(&mut self, actor: u32, exchange: &MonsterExchange, hold: Option<Hold>) {
        let ticket = Ticket {
            spawn: exchange.spawn,
            serial: exchange.serial,
            member: exchange.member,
            area: exchange.area,
        };
        if exchange.command == MonsterCommand::Confirm {
            if let Some(pending) = &mut self.pending
                && pending.ticket == ticket
            {
                pending.confirmed = true;
            }
            return;
        }
        let Some(peer) = self.peers.get_mut(usize::from(exchange.member)) else {
            return;
        };
        if peer.actor != actor {
            if let Some(held) = &mut peer.held {
                held.ended = true;
            }
            peer.actor = actor;
            peer.last_serial = None;
            peer.last_end = None;
            peer.request = None;
        }
        if exchange.command == MonsterCommand::End {
            if hold.is_some_and(|hold| hold.caster == ticket.member && hold.serial != ticket.serial)
            {
                return;
            }
            if let Some(held) = &mut peer.held
                && held.ticket == ticket
            {
                held.ended = true;
            }
            if peer
                .request
                .as_ref()
                .is_some_and(|request| request.ticket == ticket)
            {
                peer.request = None;
            }
            if peer.last_serial == Some(ticket.serial) {
                peer.last_end = Some(ticket);
                return;
            }
        }
        if peer
            .last_serial
            .is_some_and(|last| !(1..0x8000).contains(&ticket.serial.wrapping_sub(last)))
        {
            return;
        }
        peer.last_serial = Some(ticket.serial);
        if exchange.command == MonsterCommand::End {
            peer.last_end = Some(ticket);
        } else {
            peer.request = Some(Request {
                ticket,
                center: exchange.center,
                remaining: WAIT,
            });
        }
    }

    pub fn confirm(
        &mut self,
        scene: &HurtScene<'_>,
        frames: f32,
        landing: bool,
    ) -> Option<(GroundHold, Ticket)> {
        let pending = self.pending.as_mut()?;
        pending.remaining -= frames;
        let monster = scene.find_spawn(pending.ticket.spawn);
        if !landing
            || scene.area() != pending.ticket.area
            || pending.remaining <= 0.
            || monster
                .as_ref()
                .and_then(|monster| monster.network_controller())
                != Some(pending.ticket.member)
        {
            self.cancel_pending();
            return None;
        }
        if !pending.confirmed {
            return None;
        }
        let target = monster
            .filter(|monster| !monster.locally_owned())?
            .observe_ground_hold()?;
        let ticket = pending.ticket;
        self.pending = None;
        Some((target, ticket))
    }

    pub fn adopt(&mut self, hold: Hold, target: GroundHold, area: u8) {
        let Some(actor) =
            (unsafe { Roster::current() }).and_then(|roster| roster.peer(usize::from(hold.caster)))
        else {
            return;
        };
        let peer = &mut self.peers[usize::from(hold.caster)];
        if peer.held.is_some() {
            return;
        }
        if peer.actor != actor {
            *peer = Peer::new();
            peer.actor = actor;
        }
        if peer
            .last_serial
            .is_some_and(|last| !(0..0x8000).contains(&hold.serial.wrapping_sub(last)))
        {
            return;
        }
        let ticket = Ticket {
            spawn: target.spawn(),
            serial: hold.serial,
            member: hold.caster,
            area,
        };
        // The caster may cancel after GU changes the controller but before
        // this console runs its first collision update as the new controller.
        peer.held = Some(Hosted {
            ticket,
            target,
            elapsed: hold.elapsed,
            ended: peer.last_end == Some(ticket),
        });
        peer.last_serial = Some(hold.serial);
        peer.request = None;
    }

    pub fn update_controller(
        &mut self,
        scene: &HurtScene<'_>,
        frames: f32,
        in_quest: bool,
        meters: &mut [Meter],
    ) {
        let roster = unsafe { Roster::current() };
        for (member, peer) in self.peers.iter_mut().enumerate() {
            if peer.held.is_none() && peer.request.is_none() {
                continue;
            }
            let state = roster
                .as_ref()
                .and_then(|r| r.peer(member))
                .filter(|&actor| actor == peer.actor)
                .and_then(|actor| peer_action(actor, scene.area()));
            if let Some(held) = &mut peer.held {
                if !held.target.locally_owned() {
                    peer.held = None;
                } else if !held.ended
                    && held.ticket.area == scene.area()
                    && matches!(state, Some(98 | 99 | 105 | 157))
                    && held.target.active()
                {
                    held.elapsed += frames;
                } else {
                    let held = peer.held.take().unwrap();
                    held.target.release();
                    if held.elapsed > 60.
                        && let Some(meter) = meters
                            .iter_mut()
                            .find(|meter| meter.spawn == held.ticket.spawn)
                    {
                        meter.consume();
                    }
                }
            }
            let Some(mut request) = peer.request.take() else {
                continue;
            };
            request.remaining -= frames;
            if request.remaining <= 0.
                || request.ticket.area != scene.area()
                || state.is_none()
                || peer.held.is_some()
            {
                continue;
            }
            let Some(monster) = scene
                .find_spawn(request.ticket.spawn)
                .filter(|monster| monster.locally_owned())
            else {
                continue;
            };
            if matches!(state, Some(98 | 99)) {
                if !in_quest {
                    continue;
                }
                if !meters
                    .iter()
                    .any(|meter| meter.spawn == request.ticket.spawn && meter.ready())
                {
                    continue;
                }
                // A delayed position sample may lag the landing packet. Allow
                // it to catch up within one authored contact radius.
                let origin = unsafe { movement::position(peer.actor) };
                let distance_squared = origin
                    .into_iter()
                    .zip(request.center)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f32>();
                let radius = program::BIND_CONTACT.1;
                if distance_squared <= radius * radius && monster.overlaps(request.center, radius) {
                    if let Some(target) =
                        monster.try_ground_hold(Contest::monster_cap(BIND_DURATION))
                    {
                        self.outgoing
                            .push(request.ticket.packet(MonsterCommand::Confirm, [0.; 3]));
                        peer.held = Some(Hosted {
                            ticket: request.ticket,
                            target,
                            elapsed: 0.,
                            ended: false,
                        });
                    }
                    continue;
                }
            }
            if matches!(state, Some(94 | 97 | 98 | 99)) {
                peer.request = Some(request);
            }
        }
    }

    pub fn hold(&self, spawn: MonsterSpawn) -> Option<Hold> {
        let held = self
            .peers
            .iter()
            .filter_map(|peer| peer.held.as_ref())
            .find(|held| held.ticket.spawn == spawn)?;
        Some(Hold {
            caster: held.ticket.member,
            serial: held.ticket.serial,
            elapsed: held.elapsed,
        })
    }

    pub fn hosts_caster(&self, actor: u32, member: usize, area: u8) -> bool {
        self.caster_target(actor, member, area).is_some()
    }

    pub fn caster_target(&self, actor: u32, member: usize, area: u8) -> Option<MonsterSpawn> {
        let peer = self.peers.get(member)?;
        let held = peer.held.as_ref()?;
        (peer.actor == actor
            && !held.ended
            && held.ticket.area == area
            && held.target.locally_owned()
            && held.target.active())
        .then_some(held.ticket.spawn)
    }

    pub fn take_outgoing(&mut self) -> Vec<MonsterExchange> {
        core::mem::take(&mut self.outgoing)
    }
}

fn peer_action(actor: u32, area: u8) -> Option<u8> {
    if !remote::is_position_event_peer(crate::STYLE.id, actor)
        || !unsafe { hunter::area(actor) == area && hunter::is_alive(actor) }
    {
        return None;
    }
    let (group, slot) = unsafe { action::current(actor) };
    action::custom_id(group, slot)
}
