//! One-shot events in FZ 81216E36 and its w12 clip handlers.
//! Source motion frames and joint numbers are preserved here.
use super::{Lifetime, Visuals};

pub(super) fn play(v: &Visuals, clip: u16, state: u8, model_motion: u8, tier: u8, unlocked: bool) {
    let monster_guard = matches!(state, 90 | 92);
    match clip {
        1003 => {
            if v.crossed(8.0) && model_motion == 5 {
                v.joint(4015, 4, 21, [-10.0, 0.0, 60.0], Lifetime::Natural);
                v.joint(4014, 2, 21, [0.0, 0.0, 30.0], Lifetime::Natural);
            }
        }
        1009 => {
            if v.crossed(6.0) && model_motion == 5 {
                v.joint(4015, 4, 21, [-30.0, 0.0, 90.0], Lifetime::Natural);
                v.joint(4014, 2, 21, [0.0, 0.0, 30.0], Lifetime::Natural);
            }
        }
        1052 => {
            if v.crossed(12.0) {
                v.swing(v.phial.bank, 31, [0.0, 130.0, 100.0], [0.0, 0.0, 15.0], 1.1);
            }
            if v.crossed(12.0) && state == 166 {
                v.pose(v.phial.bank, 28, 0, [0.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1053 => {
            if v.crossed(12.0) {
                v.swing(v.phial.bank, 31, [0.0, 130.0, 50.0], [0.0, 0.0, 345.0], 1.1);
            }
            if v.crossed(12.0) && state == 135 {
                v.pose(v.phial.bank, 29, 0, [0.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1054 => {
            if v.crossed(12.0) {
                v.swing(
                    v.phial.bank,
                    31,
                    [0.0, 130.0, 100.0],
                    [0.0, 0.0, 345.0],
                    1.1,
                );
            }
            if v.crossed(12.0) && state == 161 {
                v.pose(v.phial.bank, 29, 0, [0.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1055 | 1479 => {
            if v.crossed(22.0) {
                v.pose(v.phial.bank, 14, 0, [0.0, 70.0, -110.0], Lifetime::Natural);
                v.rotated_pose(
                    v.phial.bank,
                    16,
                    0,
                    [0.0, 60.0, -10.0],
                    Lifetime::Natural,
                    None,
                );
            }
            if v.crossed(34.0) {
                v.pose(v.phial.bank, 16, 21, [-10.0, 0.0, 0.0], Lifetime::Natural);
            }
            if v.crossed(38.0) {
                v.pose(v.phial.bank, 16, 21, [-10.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1057 => {
            if v.crossed(24.0) {
                v.swing(
                    v.phial.bank,
                    31,
                    [0.0, 100.0, 100.0],
                    [0.0, 0.0, 267.0],
                    1.1,
                );
            }
            if v.crossed(74.0) {
                v.swing(v.phial.bank, 31, [0.0, 110.0, 0.0], [0.0, 0.0, 262.0], 1.3);
                v.pose(v.phial.bank, 30, 0, [0.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1058 => {
            if v.crossed(12.0) {
                v.swing(
                    v.phial.bank,
                    31,
                    [0.0, 120.0, -50.0],
                    [0.0, 0.0, 230.0],
                    1.1,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [0.0, 80.0, 100.0], Lifetime::Natural);
                v.pose(v.phial.bank, 16, 0, [0.0, 80.0, 50.0], Lifetime::Natural);
            }
        }
        1059 => {
            if v.crossed(12.0) {
                v.rotated_pose(
                    v.phial.bank,
                    16,
                    0,
                    [-30.0, 80.0, 50.0],
                    Lifetime::Natural,
                    Some(-0x4000),
                );
                v.swing(v.phial.bank, 31, [0.0, 110.0, 50.0], [0.0, 0.0, 110.0], 1.1);
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [-100.0, 80.0, 0.0], Lifetime::Natural);
            }
        }
        1060 => {
            if v.crossed(12.0) {
                v.rotated_pose(
                    v.phial.bank,
                    16,
                    0,
                    [30.0, 80.0, 0.0],
                    Lifetime::Natural,
                    Some(0x4000),
                );
                v.swing(
                    v.phial.bank,
                    31,
                    [0.0, 110.0, -50.0],
                    [0.0, 0.0, 250.0],
                    1.1,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [100.0, 80.0, 0.0], Lifetime::Natural);
            }
        }
        1061 => {
            if v.crossed(12.0) {
                v.swing(
                    v.phial.bank,
                    31,
                    [-20.0, 130.0, 50.0],
                    [0.0, 0.0, 15.0],
                    1.1,
                );
            }
            if v.crossed(12.0) && state == 134 {
                v.pose(v.phial.bank, 28, 0, [0.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1062 => {
            if v.crossed(12.0) {
                v.swing(
                    v.phial.bank,
                    31,
                    [-20.0, 130.0, 100.0],
                    [0.0, 0.0, 15.0],
                    1.1,
                );
            }
            if v.crossed(12.0) && state == 160 {
                v.pose(v.phial.bank, 28, 0, [0.0, 0.0, 0.0], Lifetime::Natural);
            }
        }
        1403 => {
            if v.crossed(30.0) {
                v.ground(15, [0.0, 0.0, 50.0]);
            }
        }
        1405 => {
            if v.crossed(40.0) {
                v.ground(14, [0.0, 0.0, 50.0]);
            }
            if v.crossed(78.0) {
                v.ground(14, [0.0, 0.0, 50.0]);
            }
        }
        1417 => {
            if v.crossed(2.0) {
                v.pose(4014, 26, 21, [0.0, 0.0, 30.0], Lifetime::Natural);
            }
        }
        1418 => {
            if v.crossed(2.0) {
                v.pose(v.phial.bank, 1, 0, [0.0, 120.0, 0.0], Lifetime::Hunter);
            }
        }
        1419 => {
            if v.crossed(4.0) && state == 125 {
                v.sound(2, 4);
            }
            if v.crossed(10.0) && state == 125 {
                v.rotated_pose(4015, 2, 21, [-10.0, 0.0, -60.0], Lifetime::Natural, Some(0));
                v.sound(0, 4);
            }
            if v.crossed(14.0) && state == 125 {
                v.joint(v.phial.bank, 22, 21, [-10.0, 0.0, 0.0], Lifetime::Natural);
                v.pose(v.phial.bank, 24, 0, [0.0, 0.0, 100.0], Lifetime::Natural);
            }
            if v.crossed(16.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [90.0, -160.0, 200.0],
                    [-5.0, 0.0, 15.0],
                    1.0,
                );
            }
            if v.crossed(16.0) && state == 125 {
                v.swing(
                    v.phial.bank,
                    23,
                    [90.0, -160.0, 200.0],
                    [-5.0, 0.0, 15.0],
                    1.0,
                );
            }
            if v.crossed(18.0) && state == 125 {
                v.pose(v.phial.bank, 14, 0, [0.0, 100.0, 250.0], Lifetime::Natural);
            }
        }
        1420 => {
            if v.crossed(40.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 130.0, 50.0], [0.0, 0.0, 197.0], 1.0);
                v.sound(0, 27);
            }
        }
        1421 => {
            if v.crossed(48.0) && unlocked {
                v.swing(v.phial.bank, 0, [-20.0, 150.0, 0.0], [0.0, 0.0, 20.0], 1.0);
                v.sound(0, 27);
            }
        }
        1422 => {
            if v.crossed(22.0) {
                v.joint(
                    4013,
                    v.phial.accent(2),
                    21,
                    [0.0, 0.0, 0.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(36.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 20.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(82.0) {
                v.joint(
                    4013,
                    v.phial.accent(1),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Hunter,
                );
            }
        }
        1423 => {
            if v.crossed(20.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(22.0) {
                v.joint(4014, 5, 21, [0.0, 0.0, 0.0], Lifetime::Clip1423);
                v.sound(0, 12);
                v.sound(0, 13);
            }
            if v.crossed(48.0) {
                v.joint(4014, 5, 21, [0.0, 0.0, 0.0], Lifetime::Clip1423);
            }
            if v.crossed(66.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(78.0) {
                v.joint(4014, 4, 21, [0.0, 0.0, 0.0], Lifetime::Clip1423);
            }
            if v.crossed(104.0) {
                v.joint(4014, 4, 21, [0.0, 0.0, 0.0], Lifetime::Clip1423);
            }
            if v.crossed(106.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(122.0) {
                v.joint(4014, 4, 21, [0.0, 0.0, 0.0], Lifetime::Clip1423);
            }
        }
        1425 => {
            if v.crossed(4.0) {
                v.ground(16, [0.0, 0.0, 100.0]);
            }
        }
        1429 => {
            if v.crossed(18.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 80.0, 30.0], [0.0, 0.0, 102.0], 1.0);
                v.sound(0, 27);
            }
        }
        1434 => {
            if v.crossed(30.0) {
                v.joint(
                    4013,
                    v.phial.accent(2),
                    21,
                    [0.0, 0.0, 0.0],
                    Lifetime::Hunter,
                );
            }
        }
        1437 | 1438 => {
            if v.crossed(10.0) {
                v.joint(v.phial.bank, 22, 21, [-10.0, 0.0, 0.0], Lifetime::Natural);
                v.ground(14, [0.0, 0.0, 200.0]);
            }
            if v.crossed(12.0) {
                v.pose(v.phial.bank, 24, 0, [0.0, 0.0, 100.0], Lifetime::Natural);
            }
            if v.crossed(14.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [-50.0, -100.0, 400.0],
                    [-20.0, 0.0, 345.0],
                    1.0,
                );
                v.swing(
                    v.phial.bank,
                    23,
                    [-50.0, -100.0, 400.0],
                    [-20.0, 0.0, 345.0],
                    1.0,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [0.0, 100.0, 400.0], Lifetime::Natural);
            }
        }
        1444 => {
            if v.crossed(8.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 130.0, 80.0], [0.0, 0.0, 197.0], 1.0);
                v.sound(0, 27);
            }
        }
        1445 | 1446 => {
            if v.crossed(10.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 130.0, 80.0], [0.0, 0.0, 197.0], 1.0);
                v.sound(0, 27);
            }
        }
        1447 | 1448 => {
            if v.crossed(2.0) {
                v.joint(4014, 25, 21, [0.0, 0.0, 20.0], Lifetime::HoldStart);
            }
            if v.crossed(40.0) {
                v.joint(4014, 24, 21, [0.0, 0.0, 20.0], Lifetime::Natural);
                v.sound(0, 23);
            }
        }
        1454 => {
            if v.crossed(44.0) {
                v.swing(v.phial.bank, 0, [0.0, 140.0, 100.0], [0.0, 0.0, 315.0], 1.0);
                v.ground(14, [0.0, 0.0, 50.0]);
            }
            if v.crossed(46.0) {
                v.pose(
                    v.phial.bank,
                    15,
                    21,
                    [-100.0, -60.0, 30.0],
                    Lifetime::Natural,
                );
            }
            if v.crossed(88.0) {
                v.swing(v.phial.bank, 0, [0.0, 140.0, 100.0], [0.0, 0.0, 45.0], 1.0);
                v.ground(14, [0.0, 0.0, 50.0]);
            }
            if v.crossed(92.0) {
                v.pose(
                    v.phial.bank,
                    15,
                    21,
                    [-100.0, 60.0, 30.0],
                    Lifetime::Natural,
                );
            }
        }
        1455 => {
            if v.crossed(8.0) {
                v.joint(
                    4013,
                    v.phial.accent(2),
                    21,
                    [0.0, 0.0, 0.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(68.0) {
                v.pose(
                    4013,
                    v.phial.accent(1),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(114.0) {
                v.joint(4014, 18, 21, [0.0, 0.0, 50.0], Lifetime::Hunter);
            }
            if v.crossed(130.0) {
                v.joint(4014, 18, 21, [0.0, 0.0, 50.0], Lifetime::Hunter);
            }
            if v.crossed(146.0) {
                v.joint(4014, 18, 21, [0.0, 0.0, 50.0], Lifetime::Hunter);
            }
            if v.crossed(162.0) {
                v.joint(4014, 17, 21, [0.0, 0.0, 80.0], Lifetime::Hunter);
            }
            if v.crossed(178.0) {
                v.joint(4014, 17, 21, [0.0, 0.0, 80.0], Lifetime::Hunter);
            }
            if v.crossed(194.0) {
                v.joint(4014, 17, 21, [0.0, 0.0, 80.0], Lifetime::Hunter);
            }
            if v.crossed(210.0) {
                v.joint(4014, 17, 21, [0.0, 0.0, 80.0], Lifetime::Hunter);
            }
        }
        1456 => {
            if v.crossed(24.0) {
                v.swing(v.phial.bank, 0, [0.0, 100.0, 150.0], [0.0, 0.0, 90.0], 1.0);
                v.ground(15, [0.0, 0.0, 50.0]);
            }
            if v.crossed(24.0) && tier == 3 {
                v.pose(v.phial.bank, 10, 0, [0.0, 100.0, 150.0], Lifetime::Natural);
            }
            if v.crossed(30.0) && tier >= 2 {
                v.pose(v.phial.bank, 19, 0, [0.0, 100.0, 200.0], Lifetime::Natural);
            }
            if v.crossed(70.0) {
                v.swing(v.phial.bank, 0, [0.0, 100.0, 150.0], [0.0, 0.0, 6.0], 1.0);
            }
            if v.crossed(70.0) && tier == 3 {
                v.pose(v.phial.bank, 9, 0, [0.0, 100.0, 150.0], Lifetime::Natural);
            }
            if v.crossed(72.0) && tier >= 2 {
                v.pose(
                    4013,
                    v.phial.accent(4),
                    0,
                    [0.0, 100.0, 180.0],
                    Lifetime::Natural,
                );
            }
            if v.crossed(74.0) && tier == 1 {
                v.ground(14, [0.0, 0.0, 50.0]);
            }
            if v.crossed(74.0) && tier >= 2 {
                v.ground(15, [0.0, 0.0, 50.0]);
            }
            if v.crossed(80.0) && tier < 3 {
                v.ground(0, [0.0, 0.0, 200.0]);
            }
            if v.crossed(84.0) && tier < 3 {
                v.ground(0, [0.0, 0.0, 200.0]);
            }
            if v.crossed(84.0) {
                v.joint(4014, 5, 21, [-20.0, 0.0, -70.0], Lifetime::Clip1456);
            }
            if v.crossed(96.0) {
                v.joint(4014, 4, 21, [-20.0, 0.0, -70.0], Lifetime::Clip1456);
            }
            if v.crossed(104.0) {
                v.joint(
                    4013,
                    v.phial.accent(2),
                    18,
                    [-10.0, 0.0, 10.0],
                    Lifetime::Clip1456,
                );
            }
            if v.crossed(112.0) {
                v.joint(4014, 4, 21, [-20.0, 0.0, -70.0], Lifetime::Clip1456);
            }
            if v.crossed(138.0) && tier < 3 {
                v.pose(4014, 1, 21, [10.0, 0.0, 30.0], Lifetime::Natural);
            }
            if v.crossed(142.0) && tier == 1 {
                v.pose(v.phial.bank, 2, 0, [0.0, 50.0, 230.0], Lifetime::Natural);
            }
            if v.crossed(142.0) {
                v.sound(0, 1);
            }
            if v.crossed(142.0) && tier >= 2 {
                v.pose(v.phial.bank, 3, 0, [0.0, 50.0, 230.0], Lifetime::Natural);
            }
            if v.crossed(142.0) && tier == 2 {
                v.sound(1, 78);
            }
            if v.crossed(142.0) && tier == 3 {
                v.pose(v.phial.bank, 18, 0, [0.0, 40.0, 300.0], Lifetime::Natural);
                v.pose(v.phial.bank, 17, 0, [0.0, 0.0, 300.0], Lifetime::Natural);
                v.sound(1, 79);
            }
            if v.crossed(146.0) {
                v.ground(16, [0.0, 0.0, 300.0]);
            }
        }
        1458 => {
            if v.crossed(6.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 80.0, 30.0], [0.0, 0.0, 102.0], 1.0);
                v.sound(0, 27);
            }
        }
        1468 => {
            if v.crossed(44.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 110.0, 50.0], [0.0, 0.0, 250.0], 0.9);
            }
            if v.crossed(76.0) && unlocked {
                v.swing(v.phial.bank, 0, [0.0, 100.0, 50.0], [0.0, 0.0, 240.0], 1.1);
            }
        }
        1469 => {
            if v.crossed(16.0) {
                v.pose(
                    4013,
                    v.phial.accent(1),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Hunter,
                );
            }
        }
        1472 | 1473 => {
            if v.crossed(2.0) && monster_guard {
                v.sound(0, 25);
                v.pose(4014, 21, 10, [0.0, 0.0, 0.0], Lifetime::Guard);
            }
            if v.crossed(10.0) && monster_guard {
                v.pose(v.phial.bank, 11, 0, [0.0, 130.0, 100.0], Lifetime::Guard);
            }
            if v.crossed(20.0) && monster_guard {
                v.joint(v.phial.bank, 12, 21, [0.0, 0.0, 30.0], Lifetime::Guard);
            }
            if v.crossed(34.0) && monster_guard {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 60.0],
                    Lifetime::Guard,
                );
            }
        }
        1476 => {
            if v.crossed(28.0) {
                v.pose(v.phial.bank, 1, 0, [0.0, 120.0, 0.0], Lifetime::Hunter);
            }
            if v.crossed(54.0) {
                v.joint(v.phial.bank, 7, 21, [-10.0, 0.0, 20.0], Lifetime::Hunter);
            }
            if v.crossed(68.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [-10.0, 0.0, 20.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(126.0) {
                v.swing(v.phial.bank, 0, [0.0, 100.0, 50.0], [0.0, 0.0, 188.0], 1.0);
            }
            if v.crossed(132.0) {
                v.ground(10, [0.0, 0.0, 120.0]);
            }
            if v.crossed(138.0) {
                v.ground(13, [0.0, 0.0, 120.0]);
            }
        }
        1481 => {
            // 812157C0 also submits this bank through 81817F62 on the weapon.
            if v.crossed(10.0) {
                v.joint(4013, v.phial.accent(2), 21, [0.; 3], Lifetime::Hunter);
            }
            if v.crossed(2.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 50.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(24.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [320.0, 100.0, 130.0],
                    [10.0, 0.0, 88.0],
                    0.9,
                );
            }
            if v.crossed(50.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [0.0, 0.0, 130.0],
                    Lifetime::Hunter,
                );
            }
            if v.crossed(72.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [260.0, 80.0, 140.0],
                    [2.0, 0.0, 83.0],
                    1.0,
                );
                v.swing(
                    v.phial.bank,
                    23,
                    [260.0, 80.0, 140.0],
                    [2.0, 0.0, 83.0],
                    1.0,
                );
            }
            if v.crossed(76.0) {
                v.pose(
                    4013,
                    v.phial.accent(1),
                    21,
                    [-100.0, 0.0, 100.0],
                    Lifetime::Hunter,
                );
            }
        }
        1483 => {
            if v.crossed(12.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [-230.0, 320.0, 130.0],
                    [-5.0, 0.0, 230.0],
                    1.0,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [0.0, 80.0, 100.0], Lifetime::Natural);
                v.pose(v.phial.bank, 16, 0, [0.0, 80.0, 50.0], Lifetime::Natural);
            }
        }
        1484 => {
            if v.crossed(12.0) {
                v.rotated_pose(
                    v.phial.bank,
                    16,
                    0,
                    [-30.0, 80.0, 50.0],
                    Lifetime::Natural,
                    Some(-0x4000),
                );
            }
            if v.crossed(14.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [380.0, 230.0, 140.0],
                    [-5.0, 0.0, 110.0],
                    1.0,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [-100.0, 80.0, 0.0], Lifetime::Natural);
            }
        }
        1485 => {
            if v.crossed(12.0) {
                v.rotated_pose(
                    v.phial.bank,
                    16,
                    0,
                    [30.0, 80.0, 0.0],
                    Lifetime::Natural,
                    Some(0x4000),
                );
            }
            if v.crossed(14.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [-380.0, 210.0, 140.0],
                    [5.0, 0.0, 250.0],
                    1.0,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [100.0, 80.0, 0.0], Lifetime::Natural);
            }
        }
        1486 | 1487 => {
            if v.crossed(10.0) {
                v.joint(v.phial.bank, 22, 21, [-10.0, 0.0, 0.0], Lifetime::Natural);
                v.ground(14, [0.0, 0.0, 200.0]);
            }
            if v.crossed(12.0) {
                v.pose(v.phial.bank, 24, 0, [0.0, 0.0, 100.0], Lifetime::Natural);
            }
            if v.crossed(14.0) {
                v.swing(
                    v.phial.bank,
                    13,
                    [40.0, -100.0, 400.0],
                    [-20.0, 0.0, 15.0],
                    1.0,
                );
                v.swing(
                    v.phial.bank,
                    23,
                    [40.0, -100.0, 400.0],
                    [-20.0, 0.0, 15.0],
                    1.0,
                );
            }
            if v.crossed(16.0) {
                v.pose(v.phial.bank, 14, 0, [0.0, 100.0, 400.0], Lifetime::Natural);
            }
        }
        1493 => {
            if v.crossed(34.0) {
                v.pose(v.phial.bank, 1, 0, [0.0, 120.0, 0.0], Lifetime::Hunter);
            }
            if v.crossed(42.0) {
                v.joint(v.phial.bank, 7, 21, [-10.0, 0.0, 20.0], Lifetime::Hunter);
            }
            if v.crossed(54.0) {
                v.joint(
                    4013,
                    v.phial.accent(0),
                    21,
                    [-10.0, 0.0, 20.0],
                    Lifetime::Hunter,
                );
            }
        }
        _ => {}
    }
}
