use super::{equipment, read, set};
use crate::weapon::{gu, inventory, resource, style};
use core::sync::atomic::{
    AtomicU32,
    Ordering::{Acquire, Release},
};
use mhgu_sdk::weapon::style::part_motion::{self, Motions, Pose};

#[derive(Clone, Copy)]
struct LoopSample {
    part: u32,
    clip: u32,
    body_frame: f32,
    weapon_frame: f32,
}

static LOOPS: mhgu_sdk::sync::Shared<[Option<LoopSample>; 4]> =
    mhgu_sdk::sync::Shared::new([None; 4]);

static SERVICE: part_motion::Service = part_motion::Service(bind, begin_motion);

// 951A80 also runs on props. Match the recorded hunter before touching any hunter fields.
static BLEND_OWNERS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
#[derive(Clone, Copy, Default)]
struct Transition {
    actor: u32,
    clip: u16,
    blend: f32,
    pending: bool,
    independent: bool,
    sampled: f32,
    advanced: f32,
}
static TRANSITIONS: mhgu_sdk::sync::Shared<[Option<Transition>; 4]> =
    mhgu_sdk::sync::Shared::new([None; 4]);

unsafe extern "C" fn begin_motion(actor: u32, clip: u16, blend: f32) {
    if actor != 0 && mhgu_sdk::service::on_game_thread() {
        let slot = unsafe { ((actor + 4177) as *const u8).read() } as usize;
        if let Some(owner) = BLEND_OWNERS.get(slot) {
            TRANSITIONS.with(|t| {
                t[slot] = Some(Transition {
                    actor,
                    clip,
                    blend,
                    pending: true,
                    ..Transition::default()
                })
            });
            owner.store(if blend > 0. { actor } else { 0 }, Release);
        }
    }
}

#[forge::hook(offset = 0x951a80)]
unsafe fn advance_layer(model: u32) -> u32 {
    let Some(slot) = BLEND_OWNERS
        .iter()
        .position(|owner| owner.load(Acquire) == model && model != 0)
    else {
        return original!(model);
    };
    unsafe {
        let packed = ((model + 1204) as *const u16).read();
        let remaining = f32::from_bits(read(model + 1268));
        let hold = TRANSITIONS.with(|t| {
            let Some(state) = t[slot]
                .as_mut()
                .filter(|s| s.actor == model && s.clip == packed)
            else {
                return false;
            };
            state.sampled = state.advanced;
            if remaining > 2. {
                state.advanced +=
                    f32::from_bits(read(model + 28)) * f32::from_bits(read(model + 1292));
            }
            remaining > 2.
        });
        if !hold {
            BLEND_OWNERS[slot].store(0, Release);
            return original!(model);
        }
        // Keep GU's pose interpolation running while holding its source frame,
        // as FZ 8173208A does. At the last FZ tick the new playhead advances.
        let flags = read(model + 1212);
        set(model, 1212, flags | 0x10);
        let result = original!(model);
        set(model, 1212, (read(model + 1212) & !0x10) | (flags & 0x10));
        result
    }
}

unsafe extern "C" fn bind(id: u32, motions: *const Motions) -> bool {
    let Some(entry) = style::registered_entry(id).filter(|entry| entry.kind.is_some()) else {
        return false;
    };
    let Some(motions) = (unsafe { motions.as_ref() }) else {
        return false;
    };
    if !(1..=8).contains(&motions.banks.len())
        || motions
            .banks
            .iter()
            .any(|path| !(1..64).contains(&path.to_bytes().len()))
    {
        return false;
    }
    entry
        .part_motion
        .store(core::ptr::from_ref(motions).cast_mut(), Release);
    true
}

pub(super) fn registered(equipment: inventory::RegisteredEquipment) -> Option<&'static Motions> {
    let (entry, _) = inventory::weapon_type(equipment.kind.id.get())?;
    unsafe { entry.part_motion.load(Acquire).as_ref() }
}

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global mhgu_part_track",
    ".hidden mhgu_part_track",
    ".type mhgu_part_track,%function",
    "mhgu_part_track:",
    "vmov s0,r3",
    "ldr ip,[sp]",
    "vmov s1,ip",
    "vmov.f32 s2,#1.0",
    "mov r3,#0",
    "ldr ip,[sp,#4]",
    "bx ip",
);

unsafe extern "C" {
    fn mhgu_part_track(part: u32, layer: u32, clip: u32, blend: u32, frame: u32, entry: u32);
}

unsafe fn attach(part: u32, actor: u32, socket: part_motion::Socket) {
    unsafe {
        // 3078B8 attaches the part to the logical hunter joint through virtual +72.
        if read(part + 5068) != socket.joint {
            set(part, 5068, socket.joint);
            let attach: unsafe extern "C" fn(u32, u32, u32) =
                core::mem::transmute(read(read(part) + 72));
            attach(part, actor, socket.joint);
        }
        set(part, 64, socket.offset);
        set(part, 80, socket.rotation);
        set(part, 96, [socket.scale; 3]);
        set(part, 5128, 0u32); // A future native socket must reapply its transform.
    }
}

unsafe fn part_bank(part: u32, motions: &Motions, clip: part_motion::Clip) -> Option<(u32, u32)> {
    unsafe {
        let path = *motions.banks.get(usize::from(clip.bank))?;
        let resource = resource::load(0x2125210, path)?;
        let slot = u32::from(clip.slot);
        let row = gu!(0xb49e54, resource, slot);
        if row == 0 {
            return None;
        }
        let bank = (8..16)
            .rev()
            .find(|&bank| read(part + 3844 + 4 * bank) == resource)
            .or_else(|| {
                (8..16)
                    .rev()
                    .find(|&bank| read(part + 3844 + 4 * bank) == 0)
            })?;
        if read(part + 3844 + 4 * bank) == 0 {
            gu!(0x957400, part, resource, bank);
        }
        Some((bank, row))
    }
}

unsafe fn track(part: u32, actor: u32) -> Option<()> {
    unsafe {
        let manager = read(forge::mem::text_addr() + 0x1885b58);
        if manager == 0
            || actor == 0
            || read(part + 48) != actor
            || read(actor + 9120) != part
            || read(part + 240) == 0
            || !mhgu_sdk::service::on_game_thread()
        {
            return None;
        }
        let motions = registered(equipment(manager, actor)?)?;
        let motion = gu!(0x53a058, actor);
        // 2D6634 plays 5255 with the hunter sheathed. The retail LMT sets
        // equipment event 30 while the primary weapon is in the sharpening hand.
        let drawn = if motion == 5255 {
            gu!(0x2824a8, actor, 30) != 0
        } else {
            let query: unsafe extern "C" fn(u32) -> u32 =
                core::mem::transmute(read(read(actor) + 376));
            query(actor) == 1
        };
        let pose = Pose {
            actor,
            motion,
            drawn,
        };
        let body_frame = f32::from_bits(read(actor + 1280)).max(0.);
        let body_blend = f32::from_bits(read(actor + 1268)).max(0.);
        let body_socket = motions
            .socket
            .map(|select| select(pose, body_frame, body_blend));
        let inherit = body_socket.is_none_or(|socket| socket.blend_motion);
        let mut blend = if inherit { body_blend } else { 0. };
        let clip = (motions.select)(pose);
        let Some((bank, row)) = part_bank(part, motions, clip) else {
            if let Some(socket) = body_socket {
                attach(part, actor, socket);
            }
            return None;
        };
        let slot = u32::from(clip.slot);
        let frames = read(row + 8) as f32;
        let loop_frame = read(row + 12) as f32;
        let packed = (bank << 8) | slot;
        let current = (part + 1204) as *const u16;
        let member = ((actor + 4177) as *const u8).read() as usize;
        let hunter_clip = ((actor + 1204) as *const u16).read();
        let (restart, offset) = TRANSITIONS.with(|t| {
            let Some(state) = t
                .get_mut(member)
                .and_then(Option::as_mut)
                .filter(|s| s.actor == actor && s.clip == hunter_clip)
            else {
                return (false, 0.);
            };
            let restart = core::mem::take(&mut state.pending);
            if restart {
                state.independent = !inherit;
                blend = if inherit { state.blend } else { 0. };
            }
            (restart, if state.independent { state.sampled } else { 0. })
        });
        let changed = restart || u32::from(current.read()) != packed;
        let frame = body_frame + offset;
        let end = (frames - 1.).max(0.);
        let body_end = f32::from_bits(read(actor + 1284));
        let body_restart = f32::from_bits(read(actor + 1288));
        let mut frame = if loop_frame < end
            && body_end > body_restart
            && (end != body_end || loop_frame != body_restart)
        {
            // 81732D8A advances the part independently on the hunter's delta.
            // In w13 clip 1435 the two resources have different loop lengths.
            LOOPS.with(|loops| {
                let Some(last) = loops.get_mut(member) else {
                    return frame.min(end);
                };
                let mut weapon_frame = frame;
                if let Some(prev) = last.filter(|s| !changed && s.part == part && s.clip == packed)
                {
                    let mut delta = frame - prev.body_frame;
                    if delta < 0. && frame >= body_restart {
                        delta += body_end - body_restart;
                    }
                    if delta >= 0. {
                        weapon_frame = prev.weapon_frame + delta;
                    }
                }
                if weapon_frame >= end {
                    weapon_frame = loop_frame + (weapon_frame - end) % (end - loop_frame);
                }
                *last = Some(LoopSample {
                    part,
                    clip: packed,
                    body_frame: frame,
                    weapon_frame,
                });
                weapon_frame
            })
        } else if loop_frame < end && frame >= end {
            loop_frame + (frame - end) % (end - loop_frame)
        } else {
            frame.min(end)
        };
        if let (Some(select), Some(body_socket)) = (motions.socket, body_socket) {
            let socket = if frame == body_frame {
                body_socket
            } else {
                select(pose, frame, body_blend)
            };
            attach(part, actor, socket);
            // Adjacent keys on opposite sides of a handoff use different
            // parent spaces. Never interpolate a local weapon pose across them.
            let lower = frame as u32 as f32;
            if lower < frame && select(pose, lower + 1., body_blend).joint != socket.joint {
                frame = lower;
            }
        }
        let play = forge::mem::text_addr() + 0x954640;
        // GU 308B3C feeds the part from the hunter playhead before 93AF60.
        set(part, 5060, motion);
        if changed {
            mhgu_part_track(part, 0, packed, blend.to_bits(), frame.to_bits(), play);
        }
        if (current.add(160)).read() != u16::MAX {
            mhgu_part_track(part, 1, 0xffff, 0, 0, play);
        }
        set(part, 1276, frame);
        set(part, 1212, read(part + 1212) | 0x10);
        Some(())
    }
}

#[forge::hook(offset = 0x3078b8)]
unsafe fn update(part: u32, actor: u32, kind: u32) -> u32 {
    let result = original!(part, actor, kind);
    unsafe { track(part, actor) };
    result
}

pub(super) fn install(base: u32) {
    if unsafe { part_motion::publish(&SERVICE) } {
        forge::install_hook!(base, update);
        forge::install_hook!(base, advance_layer);
    }
}
