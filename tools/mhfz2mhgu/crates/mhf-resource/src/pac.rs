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
