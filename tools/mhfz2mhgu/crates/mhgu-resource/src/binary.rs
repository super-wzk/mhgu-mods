/// Little-endian fields shared by the three Switch resource formats.
pub trait Fields {
    fn u16(&mut self, v: u16);
    fn u32(&mut self, v: u32);
    fn floats(&mut self, v: impl IntoIterator<Item = f32>);
    fn align(&mut self, n: usize);
    fn patch(&mut self, at: usize, v: u32);
}
impl Fields for Vec<u8> {
    fn u16(&mut self, v: u16) {
        self.extend(v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.extend(v.to_le_bytes());
    }
    fn floats(&mut self, v: impl IntoIterator<Item = f32>) {
        for x in v {
            self.extend(x.to_le_bytes());
        }
    }
    fn align(&mut self, n: usize) {
        self.resize(self.len().next_multiple_of(n), 0);
    }
    fn patch(&mut self, at: usize, v: u32) {
        self[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
}
