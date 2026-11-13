//! Battle route selection and playback of Frontier action requests.

/// State 0:3 has one target test (20 degrees), four ordered distance thresholds,
/// and a native random counter masked to five bits. The indices and actual
/// sequences come from the source scripts at package build, not a curated pool.
pub const CASES: usize = 2 * 2 * 5 * 2 * 32;
pub const ROUTE_WIDTH: usize = 8; // count, then up to three (group,id) pairs; final byte reserved
pub struct BattleView<'a> {
    pub thresholds: [f32; 4],
    pub decisions: &'a [u8],
    pub routes: &'a [u8],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request(pub u8, pub u8);
#[derive(Default)]
pub struct Pending {
    cursor: usize,
    end: usize,
}
impl Pending {
    pub fn clear(&mut self) {
        self.cursor = 0;
        self.end = 0;
    }
    pub fn is_empty(&self) -> bool {
        self.cursor == self.end
    }
    pub fn select(
        &mut self,
        data: &BattleView<'_>,
        enraged: bool,
        airborne: bool,
        distance: Option<f32>,
        angle: u16,
        random: u16,
    ) {
        let lane = distance
            .and_then(|d| data.thresholds.iter().position(|&t| t >= 0. && d <= t))
            .unwrap_or(4);
        let turn = usize::from(distance.is_some() && angle >= 3641);
        let idx = (((usize::from(enraged) * 2 + usize::from(airborne)) * 5 + lane) * 2 + turn) * 32
            + usize::from(random & 31);
        let route = usize::from(data.decisions[idx]) * ROUTE_WIDTH;
        self.cursor = route + 1;
        self.end = self.cursor + usize::from(data.routes[route]) * 2;
    }
    /// Consume an already selected route without reading the decision table again.
    pub fn next(&mut self, routes: &[u8]) -> Option<Request> {
        if self.is_empty() {
            return None;
        }
        let i = self.cursor;
        self.cursor += 2;
        Some(Request(routes[i], routes[i + 1]))
    }
}
