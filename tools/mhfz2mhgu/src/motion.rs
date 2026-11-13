use anyhow::{Context, Result, bail, ensure};
use glam::{Quat, Vec3, Vec4};
use mhf_resource::motion::{Channel, KeyEncoding, Keyframe, Motion, MotionArchive};
use mhf_resource::rig::{Rig, rotation};
use mhgu_resource::lmt::{Track, Usage, Writer};

struct Key {
    t: f32,
    y: f32,
    incoming: f32,
    outgoing: f32,
}
struct Curve(Vec<Key>);
impl Curve {
    fn read(c: &Channel<'_>, unit: f32) -> Result<Self> {
        let mut keys = Vec::new();
        for i in 0..usize::from(c.native_key_count()) {
            let (y, t, a, b) = match c.key(i)? {
                Keyframe::I16Quad {
                    value,
                    frame,
                    parameters: [a, b],
                } => (value as f32, frame as f32, a as f32, b as f32),
                Keyframe::F32Quad {
                    value_bits,
                    frame_bits,
                    parameter_bits: [a, b],
                } => (
                    f32::from_bits(value_bits),
                    f32::from_bits(frame_bits),
                    f32::from_bits(a),
                    f32::from_bits(b),
                ),
                _ => bail!(
                    "unsupported MOT interpolation {:#x} at {:#x}",
                    c.header.kind,
                    c.offset
                ),
            };
            ensure!(
                t.is_finite() && y.is_finite() && a.is_finite() && b.is_finite(),
                "non-finite key at {:#x}",
                c.offset
            );
            ensure!(
                keys.last().is_none_or(|k: &Key| k.t <= t),
                "unordered key times at {:#x}",
                c.offset
            );
            keys.push(Key {
                t,
                y: y * unit,
                incoming: a * unit,
                outgoing: b * unit,
            });
        }
        ensure!(
            !keys.is_empty(),
            "empty active MOT curve at {:#x}",
            c.offset
        );
        Ok(Self(keys))
    }

    fn sample(&self, f: f32) -> f32 {
        let k = &self.0;
        let i = k.partition_point(|k| k.t <= f);
        if i == 0 {
            return k[0].y;
        }
        if i == k.len() {
            return k[i - 1].y;
        }
        let (a, b) = (&k[i - 1], &k[i]);
        let dt = b.t - a.t;
        let u = (f - a.t) / dt;
        let v = 1. - u;
        (1. + 2. * u) * v * v * a.y + u * v * v * dt * a.outgoing + u * u * (3. - 2. * u) * b.y
            - u * u * v * dt * b.incoming
    }
}

pub(crate) fn bake(
    m: &Motion<'_>,
    rig: &Rig,
    tag: Option<u16>,
    root: Option<usize>,
) -> Result<(u32, Vec<Track>)> {
    let order = rig.bind(m.tracks.len(), tag, root)?;
    let kind = m.header.kind & 0xff;
    ensure!(matches!(kind, 1 | 2), "unsupported MOT kind {kind:#x}");
    let mut all = vec![];
    let mut end = 0f32;
    for tr in &m.tracks {
        let mut axes: [Option<Curve>; 9] = Default::default();
        for c in &tr.channels {
            if c.encoding() == KeyEncoding::Disabled {
                continue;
            }
            let slot = c
                .target_slot()
                .with_context(|| format!("unknown MOT channel mask at {:#x}", c.offset))?
                as usize;
            let unit = match (kind, slot) {
                (2, 3..=5) => std::f32::consts::TAU / 16384.,
                (2, _) => 1. / 16.,
                _ => 1.,
            };
            let curve = Curve::read(c, unit)?;
            end = end.max(curve.0.last().unwrap().t);
            axes[slot] = Some(curve);
        }
        all.push(axes);
    }
    ensure!(
        end.ceil() < u32::MAX as f32,
        "duration exceeds LMT's frame field"
    );
    let frames = end.ceil() as u32 + 1;
    // GU actor-space translation track (bone 0xff), independent of skeletal root tracks.
    let mut tracks = vec![Track {
        bone: 255,
        usage: Usage::RootTranslation,
        samples: vec![Vec4::W],
    }];
    for (axes, i) in all.iter().zip(order) {
        let joint = &rig.joints[i];
        for (group, (usage, base)) in [
            (Usage::Scale, joint.scale),
            (Usage::Rotation, joint.angles),
            (Usage::Translation, joint.pos),
        ]
        .into_iter()
        .enumerate()
        {
            let curves = &axes[3 * group..3 * group + 3];
            if curves.iter().all(Option::is_none) {
                continue;
            }
            let mut samples = Vec::with_capacity(frames as usize);
            let mut previous = None::<Quat>;
            for f in 0..frames {
                let v = Vec3::from_array(std::array::from_fn(|k| {
                    curves[k].as_ref().map_or(base[k], |c| c.sample(f as f32))
                }));
                let value = if usage == Usage::Rotation {
                    let mut q = rotation(v);
                    if previous.is_some_and(|p| p.dot(q) < 0.) {
                        q = -q;
                    }
                    previous = Some(q);
                    Vec4::from_array(q.to_array())
                } else {
                    v.extend(1.)
                };
                samples.push(value);
            }
            if samples.iter().all(|v| *v == samples[0]) {
                samples.truncate(1);
            }
            tracks.push(Track {
                bone: u8::try_from(i)
                    .ok()
                    .filter(|b| *b != 255)
                    .context("LMT bone index must be below 255")?,
                usage,
                samples,
            });
        }
    }
    Ok((frames, tracks))
}

pub fn convert(
    src: &[u8],
    rig: &Rig,
    group: usize,
    slot: Option<usize>,
    tag: Option<u16>,
    root: Option<usize>,
) -> Result<Vec<u8>> {
    let archive = MotionArchive::from_directory(src)?;
    let g = archive
        .groups
        .get(group)
        .with_context(|| format!("no MOT group {group}"))?;
    let mut writer = Writer::new(g.motion_offsets.len())?;
    if let Some(i) = slot {
        ensure!(
            g.motion_offsets.get(i).is_some_and(Option::is_some),
            "empty or missing MOT slot {group}:{i}"
        );
    }
    for (i, offset) in g.motion_offsets.iter().enumerate() {
        if slot.is_some_and(|selected| selected != i) {
            continue;
        }
        let Some(offset) = *offset else {
            continue;
        };
        let m = Motion::parse_at(src, offset as usize)?;
        let (frames, tracks) = bake(&m, rig, tag, root)
            .with_context(|| format!("MOT {group}:{i} at {:#x}", m.offset))?;
        writer.clip(i, frames, &tracks)?;
    }
    Ok(writer.finish())
}
