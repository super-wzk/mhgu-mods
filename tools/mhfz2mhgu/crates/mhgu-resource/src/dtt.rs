//! GU 1.4 ordinary Rathian rEnemyDtTune layout.
use anyhow::{Result, ensure};

pub struct HuntTune {
    pub hitzones: [[u8; 10]; 14],
    /// FZ gauge, duration in logic ticks, motion, attack and defense multipliers.
    pub anger: [u8; 16],
}

pub fn rathian(mut dtt: Vec<u8>, far: u8, near: u8, hunt: Option<&HuntTune>) -> Result<Vec<u8>> {
    // em001_00: 2 ground and 2 alternate focus records, then five trailer words.
    // Guard the donor layout before patching: these are not universal DTT offsets.
    ensure!(
        dtt.starts_with(b"DTT\0\x6c\xa0\x33\x01")
            && dtt.len() == 1594
            && dtt[8] == 2
            && dtt[16..19] == [2, 2, 1],
        "expected the ordinary em001_00 DTT layout"
    );
    for row in dtt[1430..1574].as_chunks_mut::<36>().0 {
        // Both alternatives are joint position + offset in actor space, not an offset
        // rotated by the joint. Radius at +32 selects near vs far and stays unchanged.
        for (at, id) in [(0, far), (16, near)] {
            row[at..at + 16].fill(0);
            row[at] = id;
        }
    }
    if let Some(tune) = hunt {
        // 57740 loads the ordinary anger block into rEnemyDtTune +140.
        // This offset is for the guarded layout, before inserting the alternate hitzones.
        let anger = &mut dtt[430..446];
        anger.copy_from_slice(&tune.anger);
        let duration = i16::from_le_bytes(tune.anger[2..4].try_into()?)
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("anger duration exceeds GU frames"))?;
        anger[2..4].copy_from_slice(&duration.to_le_bytes());
        let rows = &tune.hitzones;
        // 57740 reads a 56-byte header, 4 + 80 bytes, then header[8] bytes
        // before the default 8 x 10 table. Header[18] == 0 enables a second
        // table immediately afterwards. Keep GU's eighth row in both banks.
        let start = 56 + 4 + 80 + usize::from(dtt[8]);
        let end = start + 80;
        dtt[start..start + 70].copy_from_slice(rows[..7].as_flattened());
        let mut alternate = [0; 80];
        alternate[..70].copy_from_slice(rows[7..].as_flattened());
        alternate[70..].copy_from_slice(&dtt[start + 70..end]);
        dtt.splice(end..end, alternate);
        dtt[18] = 0;
    }
    Ok(dtt)
}
