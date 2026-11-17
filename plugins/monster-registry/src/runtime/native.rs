//! GU's factory, module loader and resource slots accept a retail layout index.
//! Record accessors must never use that layout as a player's species identity.
use super::{counts, species, with};

#[forge::hook(offset = 0x50b64)]
unsafe fn layout(raw: u32) -> i32 {
    let native = original!(raw);
    if native != -1 {
        return native;
    }
    let template = species(raw).and_then(|id| with(|s| s.registry.template(id)));
    template.map_or(-1, |id| original!(u32::from(id.packed())))
}

// The result transfer already credits added hunts through Registry::settle.
// Direct native count/size writes (including from other GU screens) must not
// modify the native template. Unknown keys keep the original miss behavior.
fn added(raw: u32) -> bool {
    species(raw).is_some_and(|id| !id.is_native() && with(|s| s.registry.contains(id)))
}

macro_rules! retail_only {
    ($name:ident, $at:literal) => {
        #[forge::hook(offset = $at)]
        unsafe fn $name(player: u32, raw: u32, value: u32) -> u32 {
            if added(raw) {
                0
            } else {
                original!(player, raw, value)
            }
        }
    };
}
retail_only!(add_slain, 0x526d94);
retail_only!(add_captured, 0x526e00);
retail_only!(largest, 0x526eb0);
retail_only!(smallest, 0x526ef8);
retail_only!(size, 0x526f48);

fn total(raw: u32) -> u32 {
    let n = counts(raw);
    if n == u32::MAX { 0 } else { n }
}
#[forge::hook(offset = 0x526dd4)]
unsafe fn slain(player: u32, raw: u32) -> u32 {
    if added(raw) {
        total(raw) & 0xffff
    } else {
        original!(player, raw)
    }
}
#[forge::hook(offset = 0x526e44)]
unsafe fn captured(player: u32, raw: u32) -> u32 {
    if added(raw) {
        total(raw) >> 16
    } else {
        original!(player, raw)
    }
}

#[forge::hook(offset = 0x523104)]
unsafe fn hunted_total(player: u32, raw: u32) -> u32 {
    if added(raw) {
        let n = total(raw);
        ((n & 0xffff) + (n >> 16)).min(9999)
    } else {
        original!(player, raw)
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, layout);
    forge::install_hook!(base, add_slain);
    forge::install_hook!(base, add_captured);
    forge::install_hook!(base, slain);
    forge::install_hook!(base, captured);
    forge::install_hook!(base, hunted_total);
    forge::install_hook!(base, largest);
    forge::install_hook!(base, smallest);
    forge::install_hook!(base, size);
}
