//! GU XFS v16 serializers for a resident encounter with no zone transitions.
use crate::binary::Fields;

/// The four route resources have the same empty RouteFindDataResource array.
/// The resource class and XFS revision are the only differences.
pub fn empty_routes(class: u32, revision: u16) -> Vec<u8> {
    const ARRAY: u32 = 0x71134d41;
    let mut b = b"XFS\0".to_vec();
    b.u16(16);
    b.u16(revision);
    for word in [2, 0, 2, 236, 8, 96] {
        b.u32(word);
    }
    let mut names = Vec::new();
    for (ty, fields) in [
        (
            class,
            [
                (b"mQuality".as_slice(), 0x49106),
                (b"RouteFindDataResource", 0x40001),
            ],
        ),
        (
            ARRAY,
            [(b"mAutoDelete".as_slice(), 0x10003), (b"mpArray", 0x4a002)],
        ),
    ] {
        b.u32(ty);
        b.u32(2);
        for (name, format) in fields {
            b.u32(184 + names.len() as u32);
            b.u32(format);
            b.extend([0; 32]);
            names.extend_from_slice(name);
            names.push(0);
        }
    }
    b.extend(names);
    b.align(4);
    // Root instance, quality=2; child owns an empty array.
    for word in [1, 33, 1, 2, 1, 0x10003, 13, 1] {
        b.u32(word);
    }
    b.push(1);
    b.u32(0);
    b
}

pub fn empty_map_info() -> Vec<u8> {
    let mut b = b"MTD\0".to_vec();
    b.u32(0x0c0122af);
    b.u32(28);
    b.resize(12 + 28 * 86, 0);
    b
}
