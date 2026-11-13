//! Switch ARC v17. A resource is identified by both its path and its MT class.
use crate::binary::Fields;
use anyhow::{Context, Result, ensure};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use std::io::{Read, Write};

pub const MODEL: u32 = 0x58a15856;
pub const MATERIAL: u32 = 0x2749c8a8;
pub const TEXTURE: u32 = 0x241f5deb;
pub const MOTION: u32 = 0x76820d81;
pub const COMMAND: u32 = 0x3aabba02;
pub struct Entry<'a> {
    pub name: &'a str,
    pub kind: u32,
    pub size: usize,
    pub compressed: &'a [u8],
}
impl Entry<'_> {
    pub fn unpack(&self) -> Result<Vec<u8>> {
        if self.compressed.len() == self.size && !self.compressed.starts_with(&[0x78]) {
            return Ok(self.compressed.to_vec());
        }
        let mut data = Vec::new();
        ZlibDecoder::new(self.compressed).read_to_end(&mut data)?;
        ensure!(
            data.len() == self.size,
            "ARC size mismatch for {}",
            self.name
        );
        Ok(data)
    }
}

pub fn entries(src: &[u8]) -> Result<Vec<Entry<'_>>> {
    ensure!(src.starts_with(b"ARC\0\x11\0"), "expected Switch ARC v17");
    let n = u16::from_le_bytes(src.get(6..8).context("short ARC header")?.try_into()?) as usize;
    src.get(12..12 + n * 80)
        .context("short ARC directory")?
        .as_chunks::<80>()
        .0
        .iter()
        .map(|r| {
            let name =
                std::str::from_utf8(&r[..r[..64].iter().position(|&x| x == 0).unwrap_or(64)])?;
            let word = |i: usize| u32::from_le_bytes(r[i..i + 4].try_into().unwrap());
            let start = word(76) as usize;
            Ok(Entry {
                name,
                kind: word(64),
                size: (word(72) & 0x1fffffff) as usize,
                compressed: src
                    .get(start..start + word(68) as usize)
                    .context("ARC entry outside file")?,
            })
        })
        .collect()
}

pub struct Asset {
    pub name: String,
    pub kind: u32,
    pub data: Vec<u8>,
}
pub fn encode(assets: &[Asset]) -> Result<Vec<u8>> {
    let mut out = b"ARC\0".to_vec();
    out.u16(17);
    out.u16(u16::try_from(assets.len())?);
    out.u32(0);
    out.resize((12 + 80 * assets.len()).next_multiple_of(0x8000), 0);
    for (i, a) in assets.iter().enumerate() {
        ensure!(
            a.name.len() < 64 && a.name.is_ascii() && !a.name.contains('\0'),
            "invalid ARC resource path {}",
            a.name
        );
        ensure!(
            a.data.len() <= 0x1fffffff,
            "resource exceeds ARC size field"
        );
        let row = 12 + 80 * i;
        out[row..row + a.name.len()].copy_from_slice(a.name.as_bytes());
        let pos = out.len();
        let mut z = ZlibEncoder::new(&mut out, Compression::default());
        z.write_all(&a.data)?;
        z.finish()?;
        out.patch(row + 64, a.kind);
        out.patch(row + 68, u32::try_from(out.len() - pos)?);
        out.patch(row + 72, 0x40000000 | a.data.len() as u32);
        out.patch(row + 76, u32::try_from(pos)?);
    }
    Ok(out)
}
