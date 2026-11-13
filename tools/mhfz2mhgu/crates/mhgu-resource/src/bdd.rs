//! Switch rBodyData: 44-byte records followed by a sentinel excluded from the count.
use crate::binary::Fields;
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy)]
pub struct Volume {
    pub bone: u16,
    pub other_bone: u16,
    pub shape: u16,
    pub hitzone: u16,
    pub part: u16,
    pub flags: u16,
    pub disabled_by: u16,
    pub extra: u16,
    pub radius: f32,
    pub start: [f32; 3],
    pub end: [f32; 3],
}

pub struct Body {
    header: [u8; 16],
    pub volumes: Vec<Volume>,
}
impl Body {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let header: [u8; 16] = bytes.get(..16).context("short BDD")?.try_into()?;
        ensure!(header.starts_with(b"BDD\0"), "not a GU BDD");
        let count = u32::from_le_bytes(header[8..12].try_into()?) as usize;
        let mut volumes = Vec::new();
        for r in bytes[16..].as_chunks::<44>().0 {
            let h = |i: usize| u16::from_le_bytes(r[i..i + 2].try_into().unwrap());
            let f = |i: usize| f32::from_le_bytes(r[i..i + 4].try_into().unwrap());
            if h(0) == u16::MAX {
                ensure!(volumes.len() == count, "BDD count differs from terminator");
                return Ok(Self { header, volumes });
            }
            volumes.push(Volume {
                bone: h(0),
                other_bone: h(2),
                shape: h(4),
                hitzone: h(6),
                part: h(8),
                flags: h(10),
                disabled_by: h(12),
                extra: h(14),
                radius: f(16),
                start: [f(20), f(24), f(28)],
                end: [f(32), f(36), f(40)],
            });
        }
        anyhow::bail!("missing BDD terminator")
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = self.header.to_vec();
        out.patch(8, self.volumes.len().try_into()?);
        for r in &self.volumes {
            for x in [
                r.bone,
                r.other_bone,
                r.shape,
                r.hitzone,
                r.part,
                r.flags,
                r.disabled_by,
                r.extra,
            ] {
                out.u16(x)
            }
            out.floats([r.radius]);
            out.floats(r.start);
            out.floats(r.end);
        }
        out.extend([255; 4]);
        out.resize(out.len() + 40, 0);
        Ok(out)
    }
}
