//! Switch enemy command resource (EMC, version 0x20120804).
use crate::binary::Fields;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
pub type Scripts = BTreeMap<(u16, u8), Vec<u8>>;

pub const END: u8 = 0xff;
pub fn action(group: u8, id: u8) -> [u8; 3] {
    [0, group, id]
}
pub fn call_action(slot: u8) -> [u8; 2] {
    [0x14, slot]
}

// A standalone 0xff is GU's group-dependent end/return instruction. 00 00 ff is
// a three-byte action instruction and appears *before* ff in some native scripts.
pub fn encode(scripts: &Scripts) -> Result<Vec<u8>> {
    const GROUPS: u16 = 21;
    ensure!(
        scripts.keys().all(|&(g, _)| g < GROUPS),
        "MHGU EMC has 21 groups"
    );
    let mut out = b"EMC\0".to_vec();
    out.u32(0x20120804);
    out.u32(GROUPS.into());
    out.u32(0);
    out.resize(16 + usize::from(GROUPS) * 16, 0);
    for g in 0..GROUPS {
        let n = scripts
            .range((g, 0)..=(g, 255))
            .next_back()
            .map_or(0, |(k, _)| usize::from(k.1) + 1);
        let table = out.len();
        let row = 16 + 16 * usize::from(g);
        out.patch(row, n as u32);
        out.patch(row + 4, u32::try_from(table)?);
        out.resize(table + 16 * n, 0);
        for i in 0..n {
            let code = scripts.get(&(g, i as u8)).map_or(&[END][..], Vec::as_slice);
            out.patch(table + 16 * i, u32::try_from(code.len())?);
            out.patch(table + 16 * i + 4, u32::try_from(out.len())?);
            out.extend(code);
            out.align(16);
        }
    }
    Ok(out)
}
