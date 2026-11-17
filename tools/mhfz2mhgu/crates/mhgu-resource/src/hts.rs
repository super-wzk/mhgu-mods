//! GU hit shapes: a directory of groups containing 48-byte shape records.
use crate::binary::Fields;
use anyhow::{Context, Result, ensure};

pub type Shape = [u8; 48];

pub struct ShapeGroups {
    header: [u8; 16],
    pub groups: Vec<Vec<Shape>>,
}

impl ShapeGroups {
    pub fn new(groups: Vec<Vec<Shape>>) -> Self {
        let mut header = [0; 16];
        header[..8].copy_from_slice(b"HTS\0\x15\x10\x13\x20");
        Self { header, groups }
    }

    pub fn read(bytes: &[u8]) -> Result<Self> {
        let header: [u8; 16] = bytes.get(..16).context("short HTS header")?.try_into()?;
        ensure!(header.starts_with(b"HTS\0"), "not a GU hit shape table");
        let count = u32::from_le_bytes(header[8..12].try_into()?) as usize;
        let directory = bytes[16..]
            .as_chunks::<8>()
            .0
            .get(..count)
            .context("short HTS directory")?;
        let mut groups = Vec::with_capacity(count);
        for entry in directory {
            let len = u32::from_le_bytes(entry[..4].try_into()?) as usize;
            let off = u32::from_le_bytes(entry[4..].try_into()?) as usize;
            let group = bytes
                .get(off..)
                .and_then(|tail| tail.as_chunks::<48>().0.get(..len))
                .context("short native HTS group")?;
            groups.push(group.to_vec());
        }
        Ok(Self { header, groups })
    }

    pub fn encode(self) -> Result<Vec<u8>> {
        let count = u32::try_from(self.groups.len())?;
        let mut out = self.header.to_vec();
        out.patch(8, count);
        out.resize(16 + 8 * self.groups.len(), 0);
        for (i, group) in self.groups.into_iter().enumerate() {
            let off = u32::try_from(out.len())?;
            out.patch(16 + 8 * i, u32::try_from(group.len())?);
            out.patch(20 + 8 * i, off);
            out.extend_from_slice(group.as_flattened());
        }
        Ok(out)
    }
}
