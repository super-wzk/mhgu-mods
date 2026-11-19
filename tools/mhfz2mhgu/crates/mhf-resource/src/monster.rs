//! Ordinary Vita monster tables selected by sub_8168ED54.
use crate::elf::Elf;
use anyhow::{Context, Result, bail, ensure};

pub struct BodyVolume {
    pub joint: u16,
    pub shape: u16,
    pub hitzone: u16,
    pub part: u16,
    pub flags: u16,
    pub disabled_by: u16,
    pub radius: f32,
    pub start: [f32; 3],
    pub end: [f32; 3],
}

pub struct Tables<'a> {
    elf: Elf<'a>,
    descriptor: u32,
}

pub struct Attack<'a> {
    pub record: &'a [u8; 40],
    pub group: u16,
    pub volumes: Vec<BodyVolume>,
}

pub struct ShellContact<'a> {
    pub record: &'a [u8; 40],
    pub radius: f32,
}

pub struct Fireball<'a> {
    pub flight: ShellContact<'a>,
    pub impact: ShellContact<'a>,
    pub speed: f32,
    pub offset: [f32; 3],
}

pub struct MotionBinding {
    pub clip: usize,
    pub blend_steps: usize,
    pub next_blend: u8,
}

impl<'a> Tables<'a> {
    pub fn espinas(src: &'a [u8]) -> Result<Self> {
        // Ordinary species 80 uses this ten-word descriptor in the supported Vita build.
        Ok(Self {
            elf: Elf::read(src)?,
            descriptor: 0x8257_ddf4 + 40 * 80,
        })
    }

    pub fn body(&self) -> Result<Vec<BodyVolume>> {
        parse(self.elf.tail(self.elf.ptr(self.descriptor, 0)?)?, false)
    }

    /// Ordinary monster melee effect, kind 1. 81DBF748 allocates the instance;
    /// 81DC008A selects the parameter descriptor and the collision directory.
    pub fn melee(&self, id: u16) -> Result<Attack<'a>> {
        let base = self.elf.ptr(0x8270_9c7c, 0)?;
        let record: &[u8; 40] = self
            .elf
            .tail(base + 40 * u32::from(id))?
            .get(..40)
            .context("short Vita attack record")?
            .try_into()?;
        let group = u16::from_le_bytes([record[16], record[17]]);
        let shapes = self.elf.ptr(0x82c7_79e8, group.into())?;
        Ok(Attack {
            record,
            group,
            volumes: parse(self.elf.tail(shapes)?, id == 54)?,
        })
    }

    /// 81CCDA3E: turn start/end, rotation, earliest entry frame and direction.
    pub fn charge_turn(&self, motion: u8) -> Result<[u16; 5]> {
        for row in self.elf.tail(0x826b95bc)?[..72].as_chunks::<12>().0 {
            let half = |i: usize| u16::from_le_bytes([row[2 * i], row[2 * i + 1]]);
            if half(0) == u16::from(motion) {
                return Ok(core::array::from_fn(|i| half(i + 1)));
            }
        }
        bail!("missing species-80 charge turn {motion}")
    }

    /// 81CE4E06 uses consecutive (frame, displacement) pairs. After the final
    /// key the vertical speed continues under gravity until terrain contact.
    pub fn flight_curves(&self, kind: usize) -> Result<[Vec<(f32, f32)>; 2]> {
        Ok([
            self.motion_curve(self.elf.ptr(0x82c70690, kind)?)?,
            self.motion_curve(self.elf.ptr(0x82c706d8, kind)?)?,
        ])
    }

    /// 81CE4C4C: hover displacement for airborne approach (2:7).
    pub fn hover_curve(&self) -> Result<Vec<(f32, f32)>> {
        self.motion_curve(self.elf.ptr(0x82c7065c, 1)?)
    }

    fn motion_curve(&self, addr: u32) -> Result<Vec<(f32, f32)>> {
        let data = self.elf.tail(addr)?;
        let mut curve = vec![];
        for row in data.as_chunks::<8>().0 {
            let frame = f32::from_le_bytes(row[..4].try_into()?);
            let pos = f32::from_le_bytes(row[4..].try_into()?);
            if frame == 0. && !curve.is_empty() {
                break;
            }
            ensure!(
                frame.is_finite()
                    && pos.is_finite()
                    && curve.last().is_none_or(|&(t, _)| frame > t),
                "invalid FZ flight curve"
            );
            curve.push((frame, pos));
        }
        ensure!(curve.len() > 1, "empty FZ flight curve");
        Ok(curve)
    }

    /// Breath 31 initializes flight row 56. On impact 81DCC628 spawns
    /// kind 9, effect 45; 81DDBD28 selects its own attack table at index 45+1.
    pub fn fireball(&self) -> Result<Fireball<'a>> {
        let contact = |table, index, shapes, joint| -> Result<ShellContact<'a>> {
            let record: &[u8; 40] = self
                .elf
                .tail(self.elf.ptr(table, 0)? + 40u32 * index)?
                .get(..40)
                .context("missing FZ shell attack")?
                .try_into()?;
            let group = u16::from_le_bytes(record[16..18].try_into()?);
            let geometry = self.elf.tail(self.elf.ptr(shapes, usize::from(group))?)?;
            let sphere = geometry.get(..42).context("short FZ shell sphere")?;
            ensure!(
                u16::from_le_bytes(sphere[..2].try_into()?) == joint
                    && sphere[2..12] == [0; 10]
                    && sphere[16..40] == [0; 24]
                    && sphere[40..42] == [255; 2],
                "unsupported shell geometry"
            );
            Ok(ShellContact {
                record,
                radius: f32::from_le_bytes(sphere[12..16].try_into()?),
            })
        };
        let offset = self.elf.tail(0x82712434 + 31 * 12)?;
        Ok(Fireball {
            flight: contact(0x827114b8, 56, 0x82c77c70, 126)?,
            impact: contact(0x827152e0, 46, 0x82c77e64, 127)?,
            speed: f32::from_bits(self.elf.ptr(0x8271221c, 31)?),
            offset: core::array::from_fn(|i| {
                f32::from_le_bytes(offset[4 * i..4 * i + 4].try_into().unwrap())
            }),
        })
    }

    /// Default incoming transition for an ordinary action with no explicit blend override.
    pub fn motion(&self, action: u8) -> Result<MotionBinding> {
        let table = self.elf.ptr(self.descriptor, 5)?;
        let row = self
            .elf
            .tail(table + 12 * u32::from(action))?
            .get(..12)
            .context("short action-to-motion record")?;
        ensure!(
            row[1] == 255,
            "motion {action} does not apply to all pose groups"
        );
        // sub_81695980 passes the signed half-duration to sub_817315F8. Positive values
        // blend the incoming pose with step 1/(n+1). Negative mode is not used here.
        let n = (row[6] as i8) / 2;
        ensure!(n >= 0, "unsupported negative transition on motion {action}");
        Ok(MotionBinding {
            clip: usize::from(row[0]),
            blend_steps: if n == 0 { 0 } else { n as usize + 1 },
            next_blend: ((row[7] as i8) / 2).max(0) as u8,
        })
    }
}

fn parse(src: &[u8], positioned: bool) -> Result<Vec<BodyVolume>> {
    let mut volumes = vec![];
    let (mut mask, mut remaining) = (0, 0);
    for row in src.as_chunks::<40>().0 {
        let h = |p: usize| u16::from_le_bytes(row[p..p + 2].try_into().unwrap());
        let f = |p: usize| f32::from_le_bytes(row[p..p + 4].try_into().unwrap());
        let joint = h(0);
        match joint {
            0xffff => {
                ensure!(remaining == 0, "unfinished body condition");
                return Ok(volumes);
            }
            // sub_816F2A60 skips this many following records when the state mask is set.
            125 => {
                ensure!(remaining == 0, "nested body condition");
                (mask, remaining) = (h(2), h(4));
            }
            0..=117 | 127 if joint != 127 || positioned => {
                ensure!(h(2) <= 1, "unknown FZ body shape {}", h(2));
                volumes.push(BodyVolume {
                    joint,
                    shape: h(2),
                    hitzone: h(4),
                    part: h(6),
                    flags: h(8),
                    disabled_by: if remaining == 0 { 0 } else { mask },
                    radius: f(12),
                    start: [f(16), f(20), f(24)],
                    end: [f(28), f(32), f(36)],
                });
                remaining = remaining.saturating_sub(1);
            }
            other => bail!("unsupported FZ body control {other}"),
        }
    }
    bail!("missing FZ body terminator")
}
