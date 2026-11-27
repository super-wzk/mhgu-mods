// Additional frame checks in Vita 81221D50, after its state and clip switches.
use super::{Event, Kind::*, Audience::*};

pub(super) fn rows(state: u8, clip: u16) -> &'static [Event] {
    match (state, clip) {
        (11 | 135, 1045) => &[
            (62, ExceptLocalBoost, World(4037, 64), 0, [0, 140, 70]),
            (62, LocalBoost, World(4037, 79), 0, [0, 110, 50]),
        ],
        (12 | 51 | 136, 1012) => &[
            (44, LocalNormal, Joint(4037, 62), 0, [-20, 120, 50]),
            (44, LocalBoost, World(4037, 57), 0, [-20, 120, 250]),
            (46, ExceptLocalNormal, World(4037, 94), 0, [0, 0, 250]),
            (46, LocalNormal, World(4037, 34), 0, [0, 0, 250]),
            (48, Everyone, Sound(17), 0, [10, 100, 150]),
            (48, LocalBoost, World(4037, 69), 0, [10, 100, 150]),
        ],
        (26 | 30 | 31 | 37 | 43 | 144, 1062) => &[
            (40, LocalBoost, Joint(4038, 17), 10, [0, 0, 0]),
            (68, LocalBoost, Joint(4037, 3), 14, [70, -50, 130]),
            (68, LocalBoost, Sound(13), 14, [70, -50, 130]),
            (78, LocalBoost, Joint(4037, 3), 14, [-30, -40, 130]),
            (104, LocalNormal, Joint(4037, 48), 0, [0, 100, 300]),
            (104, LocalBoost, Joint(4037, 58), 0, [0, 100, 300]),
        ],
        (35, 1082) => &[
            (4, Everyone, Joint(4037, 3), 14, [10, 0, 160]),
            (4, Everyone, Sound(13), 14, [10, 0, 160]),
            (10, LocalBoost, Joint(4037, 3), 0, [120, 160, 30]),
            (20, LocalBoost, Joint(4037, 3), 0, [-10, 230, -30]),
            (36, Local, Joint(4037, 21), 14, [10, 0, 80]),
            (44, Local, Joint(4037, 21), 14, [10, 0, 160]),
            (52, Local, Joint(4037, 21), 14, [10, 0, 240]),
            (60, Everyone, Joint(4037, 20), 0, [0, 130, 200]),
            (60, Peer, Joint(4037, 48), 0, [0, 130, 200]),
            (60, Everyone, Sound(23), 0, [0, 130, 200]),
            (60, LocalNormal, Joint(4037, 55), 0, [0, 130, 200]),
            (60, LocalBoost, Joint(4037, 59), 0, [0, 130, 200]),
        ],
        (38 | 53 | 138, 1013) => &[
            (44, LocalNormal, Joint(4037, 61), 0, [-30, 180, 70]),
            (46, ExceptLocalNormal, World(4037, 94), 0, [30, 0, 250]),
            (46, LocalNormal, World(4037, 34), 0, [30, 0, 250]),
            (48, Everyone, Sound(17), 0, [20, 100, 200]),
            (48, LocalBoost, World(4037, 69), 0, [20, 100, 200]),
        ],
        (39 | 146, 1424) => &[
            (20, LocalBoost, Shock, 0, [0, 0, 0]),
        ],
        (57 | 76 | 77, 1034) => &[
            (32, Everyone, Counter, 0, [0, 0, 0]),
            (32, Everyone, Joint(4037, 56), 0, [30, 140, 250]),
            (32, Everyone, Joint(4037, 20), 0, [30, 140, 250]),
            (32, Everyone, Sound(14), 0, [30, 140, 250]),
            (32, Everyone, Sound(5), 0, [30, 140, 250]),
        ],
        (58 | 61 | 62, 1472) => &[
            (10, Everyone, Part(5), 14, [60, 0, 60]),
            (10, Local, Mark, 0, [0, 0, 0]),
            (24, Local, Line, 0, [0, 0, 0]),
            (30, Everyone, Part(1), 14, [60, 0, 60]),
            (30, Everyone, Part(2), 14, [60, 0, 60]),
        ],
        (61, 1069) | (62, 1414) | (71, 1079) | (72, 1415) => &[
            (10, Everyone, Part(5), 14, [0, 0, 170]),
            (10, Local, Mark, 0, [0, 0, 0]),
            (24, Local, Line, 0, [0, 0, 0]),
            (30, Everyone, Part(1), 14, [0, 0, 170]),
            (30, Everyone, Part(2), 14, [0, 0, 150]),
        ],
        (68 | 71 | 72, 1460) => &[
            (10, Everyone, Part(5), 14, [0, 0, 170]),
            (10, Local, Mark, 0, [0, 0, 0]),
            (24, Local, Line, 0, [0, 0, 0]),
            (30, Everyone, Part(1), 14, [0, 0, 170]),
            (30, Everyone, Part(2), 14, [0, 0, 150]),
        ],
        (94 | 97, 1421) => &[
            (30, Everyone, World(4037, 40), 0, [0, 200, 100]),
            (30, Everyone, Joint(4037, 26), 0, [0, 50, 100]),
            (30, Everyone, Joint(4037, 28), 0, [0, 0, 100]),
        ],
        (95 | 159, 1032) | (96 | 160, 1031) => &[
            (8, Everyone, LateralImpact, 0, [0, 0, 0]),
        ],
        (105 | 157, 1488) => &[
            (114, Everyone, Sound(26), 14, [90, 0, 90]),
            (184, Everyone, Sound(27), 0, [0, 0, 170]),
            (184, Everyone, Joint(4037, 93), 0, [0, 0, 170]),
        ],
        (114 | 115, 1410) => &[
            (30, LocalBoost, Joint(4037, 61), 0, [-30, 160, 90]),
            (34, ExceptLocalBoost, World(4037, 94), 0, [0, 0, 300]),
            (34, Peer, Sound(17), 0, [0, 0, 300]),
            (34, LocalNormal, World(4037, 48), 0, [0, 120, 300]),
            (34, Local, Sound(17), 0, [0, 120, 300]),
            (34, LocalBoost, World(4037, 58), 0, [0, 120, 300]),
        ],
        (152, 1406) | (154, 1446) | (155, 1418) => &[
            (6, LocalBoost, Joint(4038, 17), 10, [0, 0, 0]),
            (20, LocalBoost, World(4037, 3), 0, [70, 80, 0]),
            (30, LocalBoost, World(4037, 3), 0, [-60, 140, 0]),
        ],
        (152 | 154 | 155, 1035) => &[
            (38, Boosted, AdvancingImpact, 0, [0, 0, 0]),
        ],
        _ => &[],
    }
}
