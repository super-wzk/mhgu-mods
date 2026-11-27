//! Offline comparison of the source MOT/FSKL and the shipped LMT/MOD/HTS.
use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3, Vec4};
use mhf_resource::{
    motion::{KeyEncoding, Keyframe, Motion, MotionArchive},
    pac,
    rig::{Rig, rotation},
};
use mhgu_resource::{arc, hts::ShapeGroups};
use std::{collections::BTreeMap, env, fs, path::Path};

fn word(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}
fn half(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap()) as usize
}
fn floats<const N: usize>(b: &[u8], at: usize) -> [f32; N] {
    std::array::from_fn(|i| f32::from_le_bytes(b[at + i * 4..at + i * 4 + 4].try_into().unwrap()))
}
#[derive(Clone, Copy)]
struct Pose {
    scale: Vec3,
    rot: Quat,
    pos: Vec3,
}
impl Pose {
    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }
}
struct Skeleton {
    ids: Vec<u8>,
    parents: Vec<Option<usize>>,
    rest: Vec<Pose>,
}
impl Skeleton {
    fn model(b: &[u8]) -> Self {
        assert_eq!(&b[..4], b"MOD\0");
        let n = half(b, 6);
        let at = word(b, 36);
        let mut rig = Self {
            ids: vec![],
            parents: vec![],
            rest: vec![],
        };
        for i in 0..n {
            rig.ids.push(b[at + 24 * i]);
            let parent = b[at + 24 * i + 1];
            rig.parents.push((parent != 255).then_some(parent as usize));
            let local = Mat4::from_cols_array(&floats(b, at + 24 * n + 64 * i));
            let (scale, rot, pos) = local.to_scale_rotation_translation();
            rig.rest.push(Pose { scale, rot, pos });
        }
        rig
    }
}
fn globals(locals: &[Pose], parents: &[Option<usize>], root: Mat4) -> Vec<Mat4> {
    let mut out = Vec::with_capacity(locals.len());
    for (i, p) in parents.iter().enumerate() {
        assert!(p.is_none_or(|p| p < i));
        out.push(p.map_or(root, |p| out[p]) * locals[i].matrix());
    }
    out
}
type Key = (f32, f32, f32, f32); // frame, value, incoming and outgoing slope
struct Curve {
    joint: usize,
    axis: usize,
    keys: Vec<Key>,
}
struct Original<'a> {
    rig: &'a Rig,
    curves: Vec<Curve>,
    frames: usize,
}
impl<'a> Original<'a> {
    fn new(rig: &'a Rig, layers: &[(Motion<'_>, u16)]) -> Result<Self> {
        let mut curves = vec![];
        let mut last = 0.;
        for (motion, tag) in layers {
            let order = rig.bind(motion.tracks.len(), Some(*tag), Some(0))?;
            for (track, joint) in motion.tracks.iter().zip(order) {
                for ch in &track.channels {
                    if ch.encoding() == KeyEncoding::Disabled {
                        continue;
                    }
                    let axis = ch.target_slot().unwrap() as usize;
                    let unit = if motion.header.kind & 255 == 1 {
                        1.
                    } else if (3..6).contains(&axis) {
                        std::f32::consts::TAU / 16384.
                    } else {
                        1. / 16.
                    };
                    let mut keys = vec![];
                    for i in 0..usize::from(ch.native_key_count()) {
                        let (t, y, left, right) = match ch.key(i)? {
                            Keyframe::I16Quad {
                                frame,
                                value,
                                parameters: [l, r],
                            }
                            | Keyframe::Mixed12 {
                                unknown_00: 0x20000,
                                frame,
                                value,
                                parameters: [l, r],
                            } => (frame as f32, value as f32, l as f32, r as f32),
                            Keyframe::F32Quad {
                                frame_bits,
                                value_bits,
                                parameter_bits: [l, r],
                            } => (
                                f32::from_bits(frame_bits),
                                f32::from_bits(value_bits),
                                f32::from_bits(l),
                                f32::from_bits(r),
                            ),
                            k => anyhow::bail!("unsupported source curve {k:?}"),
                        };
                        keys.push((t, y * unit, left * unit, right * unit));
                        last = f32::max(last, t);
                    }
                    curves.push(Curve { joint, axis, keys });
                }
            }
        }
        Ok(Self {
            rig,
            curves,
            frames: last.ceil() as usize + 1,
        })
    }
    fn sample(&self, frame: usize) -> Vec<Pose> {
        let mut axes: Vec<_> = self
            .rig
            .joints
            .iter()
            .map(|j| [j.scale, j.angles, j.pos])
            .collect();
        for c in &self.curves {
            let f = frame as f32;
            let i = c.keys.partition_point(|k| k.0 <= f);
            let y = if i == 0 {
                c.keys[0].1
            } else if i == c.keys.len() {
                c.keys[i - 1].1
            } else {
                let (t, a, _, da) = c.keys[i - 1];
                let (end, b, db, _) = c.keys[i];
                let dt = end - t;
                let u = (f - t) / dt;
                a + u
                    * (dt * da
                        + u * (3. * (b - a) - dt * (2. * da + db)
                            + u * (2. * (a - b) + dt * (da + db))))
            };
            axes[c.joint][c.axis / 3][c.axis % 3] = y;
        }
        axes.into_iter()
            .map(|[scale, e, pos]| Pose {
                scale,
                rot: rotation(e),
                pos,
            })
            .collect()
    }
}
struct Clip<'a> {
    bank: &'a [u8],
    directory: usize,
}
impl<'a> Clip<'a> {
    fn at(bank: &'a [u8], slot: usize) -> Option<Self> {
        if slot >= half(bank, 6) {
            return None;
        }
        let directory = word(bank, 8 + 4 * slot);
        (directory != 0).then_some(Self { bank, directory })
    }
    fn frames(&self) -> usize {
        word(self.bank, self.directory + 8)
    }
    fn sample(&self, rig: &Skeleton, frame: usize) -> (Vec<Pose>, Vec3) {
        let b = self.bank;
        let at = self.directory;
        let mut pose = rig.rest.clone();
        let mut root = Vec3::ZERO;
        for i in 0..word(b, at + 4) {
            let t = word(b, at) + 36 * i;
            let ty = b[t];
            let usage = b[t + 1];
            let bone = b[t + 3];
            let p = match ty {
                1 | 2 => Vec4::from_array(floats(b, t + 16)),
                4 | 6 => {
                    let off = word(b, t + 12) + 8 * frame.min(word(b, t + 8) / 8 - 1);
                    if ty == 6 {
                        let v = u64::from_le_bytes(b[off..off + 8].try_into().unwrap());
                        Vec4::from_array([42, 28, 14, 0].map(|shift| {
                            let n = ((v >> shift) & 0x3fffu64) as i32;
                            (if n & 8192 != 0 { n - 16383 } else { n }) as f32 / 4095.75
                        }))
                    } else {
                        let r = word(b, t + 32);
                        let min = Vec4::from_array(floats(b, r + 16));
                        let range = Vec4::from_array(floats(b, r));
                        min + range
                            * Vec4::new(
                                half(b, off) as f32,
                                half(b, off + 2) as f32,
                                half(b, off + 4) as f32,
                                0.,
                            )
                            / 65535.
                    }
                }
                _ => panic!("unexpected LMT track {ty}"),
            };
            if bone == 255 {
                assert_eq!(usage, 4);
                root = p.truncate();
                continue;
            }
            let j = rig.ids.iter().position(|id| *id == bone).unwrap();
            match usage {
                0 => pose[j].rot = Quat::from_array(p.to_array()).normalize(),
                1 => pose[j].pos = p.truncate(),
                2 => pose[j].scale = p.truncate(),
                _ => panic!("usage {usage}"),
            }
        }
        (pose, root)
    }
}
fn archives(p: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let b = fs::read(p)?;
    arc::entries(&b)?
        .into_iter()
        .filter(|e| matches!(e.kind, 0x58a15856 | 0x76820d81 | 0x5653c1b0))
        .map(|e| Ok((e.name.to_owned(), e.unpack()?)))
        .collect()
}
fn decoded(p: &Path) -> Result<Vec<u8>> {
    pac::decode(fs::read(p)?)
}
fn report(worst: &mut (f32, String), a: Vec3, b: Vec3, info: impl FnOnce() -> String) {
    let d = a.distance(b);
    assert!(d.is_finite());
    if d > worst.0 {
        *worst = (d, info());
    }
}
fn check_action_tables(dat: &Path, sdt: &[u8], program: &[u8]) -> Result<()> {
    let actions = decoded(&dat.join("mhfdat.bin"))?;
    let weapon = word(&actions, 0x614) + 13 * 8;
    let states = word(&actions, weapon);
    assert_eq!(states, half(program, 6));
    let records = word(&actions, weapon + 4);
    for state in 0..states {
        for (field, stride) in [12, 40, 12].into_iter().enumerate() {
            let at = 24 * state + 8 * field;
            let count = word(&actions, records + at);
            assert_eq!(count, word(program, 16 + at));
            if count > 0 {
                let source = word(&actions, records + at + 4);
                let dest = word(program, 20 + at);
                let len = count * stride;
                assert_eq!(
                    &actions[source..source + len],
                    &program[dest..dest + len],
                    "state {state} field {field}"
                );
            }
        }
    }
    let effects = program.windows(4).position(|b| b == b"MSFX").unwrap();
    for (bank, count, dest) in [
        (9, word(program, 8), word(program, 12)),
        (10, word(program, effects + 4), effects + 8),
    ] {
        let source = word(sdt, bank * 28 + 8);
        assert_eq!(count, half(sdt, bank * 28 + 4));
        assert_eq!(
            &sdt[source..source + count * 40],
            &program[dest..dest + count * 40]
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires MHFZ_DAT, MHGU_ROMFS and the packaged magnet_spike.arc"]
fn every_motion_frame() -> Result<()> {
    let dat = std::path::PathBuf::from(env::var("MHFZ_DAT")?);
    let gu = std::path::PathBuf::from(env::var("MHGU_ROMFS")?);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let packed = archives(&repo.join(
        "plugins/magnet-spike/dist/magnet-spike/romfs/nativeNX/arc/frontier/magnet_spike.arc",
    ))?;
    let mot = decoded(&dat.join("motion/w13.mot"))?;
    let part = decoded(&dat.join("motion/w13w.mot"))?;
    let mot = MotionArchive::from_directory(&mot)?;
    let part = MotionArchive::from_directory(&part)?;
    let weapons = fs::read(dat.join("extend/wk000.abn"))?;
    let wk = pac::decode(pac::mha_member(&weapons, "wk451.bin")?.to_vec())?;
    let rig = Rig::parse(&pac::member(&wk, 1)?)?;
    let part_gu = Skeleton::model(&packed[r"weapon\frontier\magnet_spike\wk451"]);
    let part_parents: Vec<_> = rig.joints.iter().map(|j| j.parent).collect();
    let sdt = decoded(&dat.join("mhfsdt.bin"))?;
    let program = fs::read(repo.join("plugins/magnet-spike/target/assets/magnet_spike.fmsa"))?;
    check_action_tables(&dat, &sdt, &program)?;
    let mut probes = vec![];
    for (bank, selector, name) in
        (1..=8)
            .map(|i| (9, i, format!("melee_{i}")))
            .chain([(10, 8, "bind_finisher".into())])
    {
        let dir = word(&sdt, bank * 28 + 16);
        let mut at = word(&sdt, dir + 32 * selector);
        let hts = ShapeGroups::read(&packed[&format!(r"player\hit\frontier_ms\{name}")])?;
        for shape in &hts.groups[0] {
            let node = half(&sdt, at);
            let gu_node = half(shape, 0);
            if word(&sdt, at + 8) == 8 {
                for (src, dest) in [(16, 16), (28, 32)] {
                    probes.push((
                        node,
                        gu_node & 255,
                        Vec3::from_array(floats(&sdt, at + src)),
                        Vec3::from_array(floats(shape, dest)),
                    ));
                }
            } else {
                assert_eq!((node, gu_node), (0, 255));
            }
            at += 40;
        }
        assert_eq!(half(&sdt, at), 65535);
    }
    let mut worst = (0., String::new());
    let mut socket_error = (0., String::new());
    let mut frames = 0;
    let mut clips = 0;
    for sex in ["m", "f"] {
        let raw = decoded(&dat.join(format!("parts/{sex}00/{sex}_editpl.bin")))?;
        let hunter = Rig::parse(&pac::member(&raw, 1)?)?;
        let parents: Vec<_> = hunter.joints.iter().map(|j| j.parent).collect();
        let bodies = archives(&gu.join(format!("nativeNX/arc/player/com/{sex}/001/body.arc")))?;
        let body = Skeleton::model(bodies.values().next().context("missing GU body")?);
        for group in [0, 2, 4] {
            let bank = &packed[&format!(r"player\mot\frontier_ms\{group}")];
            let weapon_bank = &packed[&format!(r"weapon\frontier\magnet_spike\mot\{group}")];
            for slot in 0..mot.groups[group].motion_offsets.len() {
                let Some(lo) = mot.motion(group, slot)? else {
                    continue;
                };
                let hi = mot.motion(group + 1, slot)?.unwrap();
                let loop_point = if lo.metadata_present == 1 {
                    f32::from_bits(lo.metadata) as i32
                } else {
                    -1
                };
                let original = Original::new(&hunter, &[(lo, 0), (hi, 1)])?;
                let clip = Clip::at(bank, slot + if sex == "f" { 128 } else { 0 })
                    .or_else(|| Clip::at(bank, slot))
                    .unwrap();
                assert_eq!(word(clip.bank, clip.directory + 12) as i32, loop_point);
                assert_eq!(
                    clip.frames(),
                    original.frames + 2 * usize::from(loop_point >= 0)
                );
                let source_part = part.motion(group, slot)?.context("missing w13w motion")?;
                let part_loop = if source_part.metadata_present == 1 {
                    f32::from_bits(source_part.metadata) as i32
                } else {
                    -1
                };
                let weapon = Original::new(&rig, &[(source_part, 0)])?;
                let part_clip =
                    Clip::at(weapon_bank, slot).context("missing packaged w13w clip")?;
                assert_eq!(
                    word(part_clip.bank, part_clip.directory + 12) as i32,
                    part_loop
                );
                assert_eq!(
                    part_clip.frames(),
                    weapon.frames + 2 * usize::from(part_loop >= 0)
                );
                clips += 1;
                // Compare every authored frame, including any longer part tail.
                let samples = clip.frames().max(part_clip.frames());
                for f in 0..samples {
                    let src = globals(&original.sample(f), &parents, Mat4::IDENTITY);
                    let (dst, root) = clip.sample(&body, f);
                    let dst = globals(&dst, &body.parents, Mat4::from_translation(root));
                    let pf = f.min(part_clip.frames() - 1);
                    let sw = globals(&weapon.sample(pf), &part_parents, Mat4::IDENTITY);
                    let (wp, wr) = part_clip.sample(&part_gu, pf);
                    let dw = globals(&wp, &part_gu.parents, Mat4::from_translation(wr));
                    let back = Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.875),
                        Quat::from_euler(glam::EulerRot::ZYX, 0.02792527, -3.1206489, 1.3962635),
                        Vec3::new(0., -8.5, -45.6),
                    );
                    for (node, id, adjust) in [
                        (14, 8, Mat4::IDENTITY),
                        (18, 12, Mat4::IDENTITY),
                        (10, 2, Mat4::IDENTITY),
                        (10, 2, back),
                    ] {
                        let source_socket = src[node] * adjust;
                        let socket = dst[body.ids.iter().position(|&v| v == id).unwrap()] * adjust;
                        let info = || {
                            format!(
                                "{sex} clip {} frame {f} socket {node}",
                                1000 + 100 * group + slot
                            )
                        };
                        report(
                            &mut socket_error,
                            source_socket.w_axis.truncate(),
                            socket.w_axis.truncate(),
                            info,
                        );
                        for &(a, b, x, y) in &probes {
                            report(
                                &mut worst,
                                (source_socket * sw[a]).transform_point3(x),
                                (socket * dw[b]).transform_point3(y),
                                info,
                            );
                        }
                    }
                    frames += 1;
                }
            }
        }
    }
    println!(
        "{clips} sex/clip pairs, {frames} frames. Worst socket {:?}, hit endpoint {:?}",
        socket_error, worst
    );
    assert!(worst.0 < 0.65 && socket_error.0 < 0.10);
    Ok(())
}

/// Independently decode both 8-byte w13 directories and compare the shipped rows,
/// including same-frame ordering, silence, and all four weighted alternatives.
#[test]
#[ignore = "requires MHFZ_DAT and the packaged magnetic media tracks"]
fn every_motion_media_row() -> Result<()> {
    let dat = std::path::PathBuf::from(env::var("MHFZ_DAT")?);
    let source = decoded(&dat.join("mhfdat.bin"))?;
    let model = half(&source, word(&source, 124) + 52 * 16881);
    assert_eq!(model, 6451);
    assert_eq!(half(&source, word(&source, 0x724) + 2 * model), 6416);
    let reach = usize::from(source[word(&source, 124) + 52 * 16881 + 28]);
    let sphere = word(&source, word(&source, 216) + 3 * 32 + 4 * reach);
    let bounds = floats::<6>(&source, sphere + 16);
    assert_eq!(
        [
            (bounds[0] + bounds[3]) * 0.5,
            (bounds[1] + bounds[4]) * 0.5,
            (bounds[2] + bounds[5]) * 0.5
        ],
        [0., 0., 125.]
    );
    let contact = word(&source, 220) + 6 * reach;
    assert_eq!(
        [
            half(&source, contact),
            half(&source, contact + 2),
            half(&source, contact + 4)
        ],
        [44, 0, 123]
    );
    let collisions = decoded(&dat.join("mhfsdt.bin"))?;
    let velocities = word(&collisions, 10 * 28 + 20);
    for rank in 0..3 {
        assert_eq!(floats::<1>(&collisions, velocities + 32 * rank), [40.]);
    }

    let assets =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/magnet-spike/target/assets");
    let sounds = fs::read(assets.join("frontier_ms_sound.flse"))?;
    let particles = fs::read(assets.join("frontier_ms_motion_fx.bin"))?;
    let mut source_fx = Vec::new();
    let mut native_se = vec![Vec::<[u8; 12]>::new(); 500];
    let mut suppressed = [0; 2];
    for (tab, descriptor, stride) in [(0, 0x618, 16), (1, 0x61c, 22)] {
        let directory = word(&source, descriptor) + 14 * 8;
        let first = word(&source, directory + 4);
        for i in 0..word(&source, directory) {
            let entry = first + 8 * i;
            let clip = half(&source, entry);
            let start = word(&source, entry + 4);
            for j in 0..half(&source, entry + 2) {
                let at = start + j * stride;
                let row = &source[at..at + stride];
                if tab == 0 {
                    if half(row, 2) == 8 {
                        suppressed[0] += 1;
                        continue;
                    }
                    source_fx.extend_from_slice(&(clip as u16).to_le_bytes());
                    source_fx.extend_from_slice(row);
                } else {
                    if half(row, 4) == 1 {
                        suppressed[1] += 1;
                        continue;
                    }
                    assert_eq!(half(row, 4), 0);
                    let mut decoded = [0u8; 12];
                    decoded[..2].copy_from_slice(&row[..2]);
                    decoded[2] = match half(row, 2) {
                        0 => 1,
                        1 => 3,
                        3 | 4 => 2,
                        5 => 4,
                        6 => 0,
                        other => panic!("w13 sound source {other}"),
                    };
                    let mut choices = 0;
                    for option in row[6..].chunks_exact(4) {
                        let weight = half(option, 2);
                        if weight > 0 {
                            decoded[4 + 2 * choices] = half(option, 0).min(255) as u8;
                            decoded[5 + 2 * choices] = u8::try_from(weight)?;
                            choices += 1;
                        }
                    }
                    if choices == 0 {
                        choices = 1;
                        decoded[4] = half(row, 6).min(255) as u8;
                        decoded[5] = 1;
                    }
                    decoded[3] = choices as u8;
                    native_se[clip - 1000].push(decoded);
                }
            }
        }
    }
    assert_eq!(suppressed, [304, 1]);
    assert_eq!(source_fx.len(), 962 * 18);
    assert_eq!(particles, source_fx);
    assert_eq!(native_se.iter().map(Vec::len).sum::<usize>(), 1313);
    for (clip, expected) in native_se.iter_mut().enumerate() {
        expected.sort_by_key(|r| half(r, 0));
        let offset = half(&sounds, 4 * clip);
        assert_eq!(half(&sounds, 4 * clip + 2), expected.len());
        assert_eq!(
            &sounds[offset..offset + 12 * expected.len()],
            expected.concat(),
            "clip {}",
            clip + 1000
        );
    }
    Ok(())
}
