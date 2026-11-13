//! Ordinary Rathian DTT focus blocks, read by GU 0xc83e4 and the target reticle at 0x61ede4.
use anyhow::{Result, ensure};

pub fn rathian(mut dtt: Vec<u8>, far: u8, near: u8) -> Result<Vec<u8>> {
    // em001_00: 2 ground and 2 alternate focus records, then five trailer words.
    // Guard the donor layout before patching: these are not universal DTT offsets.
    ensure!(
        dtt.starts_with(b"DTT\0\x6c\xa0\x33\x01") && dtt.len() == 1594 && dtt[16..18] == [2, 2],
        "expected the ordinary em001_00 DTT focus layout"
    );
    for row in dtt[1430..1574].as_chunks_mut::<36>().0 {
        // Both alternatives are joint position + offset in actor space, not an offset
        // rotated by the joint. Radius at +32 selects near vs far and stays unchanged.
        for (at, id) in [(0, far), (16, near)] {
            row[at..at + 16].fill(0);
            row[at] = id;
        }
    }
    Ok(dtt)
}
