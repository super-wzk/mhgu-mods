use crate::read_u32 as word;
use anyhow::{Context, Result, ensure};

struct Segment<'a> {
    va: u32,
    data: &'a [u8],
    executable: bool,
}
pub(crate) struct Elf<'a>(Vec<Segment<'a>>);
fn half(src: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        src.get(p..p + 2)
            .context("truncated ELF field")?
            .try_into()?,
    ))
}
impl<'a> Elf<'a> {
    pub(crate) fn read(src: &'a [u8]) -> Result<Self> {
        ensure!(
            src.starts_with(b"\x7fELF\x01\x01") && half(src, 18)? == 40,
            "expected a little-endian ARM32 Vita ELF"
        );
        let ph = word(src, 28)? as usize;
        let size = usize::from(half(src, 42)?);
        let mut segs = vec![];
        for i in 0..usize::from(half(src, 44)?) {
            let o = ph + i * size;
            if word(src, o)? != 1 {
                continue;
            }
            let start = word(src, o + 4)? as usize;
            let len = word(src, o + 16)? as usize;
            segs.push(Segment {
                va: word(src, o + 8)?,
                data: src
                    .get(start..start + len)
                    .context("truncated ELF load segment")?,
                executable: word(src, o + 24)? & 1 != 0,
            });
        }
        Ok(Self(segs))
    }
    pub(crate) fn executable_segments(&self) -> impl Iterator<Item = &[u8]> {
        self.0.iter().filter(|s| s.executable).map(|s| s.data)
    }
    pub(crate) fn tail(&self, va: u32) -> Result<&'a [u8]> {
        self.0
            .iter()
            .find_map(|s| {
                let delta = va.checked_sub(s.va)? as usize;
                s.data.get(delta..).filter(|x| !x.is_empty())
            })
            .with_context(|| format!("{va:#010x} is not in a file-backed ELF segment"))
    }
    pub(crate) fn ptr(&self, base: u32, index: usize) -> Result<u32> {
        Ok(word(self.tail(base)?, index * 4)?)
    }
}
