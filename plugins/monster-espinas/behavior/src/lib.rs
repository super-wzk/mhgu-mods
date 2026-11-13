//! Ordinary Frontier species 80. Motion numbers and request numbers belong to FZ.
#![no_std]

mod battle;
mod spawn;

pub use battle::{BattleView, CASES, Pending, ROUTE_WIDTH, Request};
pub use spawn::{Spawn, SpawnPhase, Step};
