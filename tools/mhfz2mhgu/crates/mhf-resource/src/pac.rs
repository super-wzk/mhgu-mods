//! Frontier PAC directories and their nested resource envelopes.
use crate::{
    crypto::{Ecd, Exf},
    jkr::Jkr,
    read_u32 as word,
};
use anyhow::{Context, Result, ensure};
pub fn decode(mut data: Vec<u8>) -> Result<Vec<u8>> {
    loop {
        data = match data.get(..4) {
            Some(b"ecd\x1a") => Ecd::parse(&data)?.decode()?.into_inner(),
            Some(b"exf\x1a") => Exf::parse(&data)?.decode()?.into_inner(),
            Some(b"JKR\x1a") => Jkr::parse(&data)?.decode()?.into_inner(),
            _ => return Ok(data),
        }
        .into_vec();
    }
}

pub fn member(bytes: &[u8], index: usize) -> Result<Vec<u8>> {
    let count = word(bytes, 0)? as usize;
    ensure!(
        count <= (bytes.len() - 4) / 8 && index < count,
        "PAC member {index} outside directory of {count}"
    );
    let a = word(bytes, 4 + 8 * index)? as usize;
    let len = word(bytes, 8 + 8 * index)? as usize;
    ensure!(
        len > 0 && a >= 4 + 8 * count,
        "empty or invalid PAC member {index}"
    );
    let part = bytes
        .get(a..)
        .and_then(|s| s.get(..len))
        .context("PAC member exceeds file")?;
    decode(part.to_vec()).with_context(|| format!("decode PAC member {index}"))
}

/// Find an exact member in a Frontier MHA archive. Several names may share a payload.
pub fn mha_member<'a>(bytes: &'a [u8], name: &str) -> Result<&'a [u8]> {
    ensure!(
        bytes.starts_with(b"mha\x01"),
        "expected a Frontier MHA archive"
    );
    let table = word(bytes, 4)? as usize;
    let count = word(bytes, 8)? as usize;
    let names = bytes
        .get(word(bytes, 12)? as usize..)
        .context("invalid MHA names")?;
    let rows = bytes
        .get(table..)
        .and_then(|s| s.get(..count.checked_mul(20)?))
        .context("short MHA directory")?;
    for row in rows.chunks_exact(20) {
        let field = names
            .get(word(row, 0)? as usize..)
            .context("invalid MHA name")?;
        if field
            .strip_prefix(name.as_bytes())
            .is_some_and(|s| s.first() == Some(&0))
        {
            return bytes
                .get(word(row, 4)? as usize..)
                .and_then(|s| s.get(..word(row, 8).ok()? as usize))
                .context("short MHA member");
        }
    }
    anyhow::bail!("missing MHA member {name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_nested_jkr_until_payload() {
        let mut input = b"monster".to_vec();
        for _ in 0..12 {
            let mut w = b"JKR\x1a\x08\x01\0\0".to_vec();
            w.extend(16u32.to_le_bytes());
            w.extend((input.len() as u32).to_le_bytes());
            w.extend(input);
            input = w;
        }
        assert_eq!(decode(input).unwrap(), b"monster");
    }
}
