//! Frontier's release gauge and four blade-polish marks on GU's twin-sword HUD.
use crate::{battle, local_actor, native, program, read, write};
use mhgu_sdk::style::hud::GaugePlate;
static PLATE: GaugePlate = GaugePlate::new(
    0x4000_0002,
    c"GUI\\frontier_db\\gauge",
    [384., 112., 512., 136.],
);
#[forge::hook(offset = 0x64b830)]
unsafe fn twin_sword_gauge(widget: u32) -> u32 {
    unsafe {
        let layout = read::<u32>(widget, 36);
        let Some(actor) = local_actor().filter(|&actor| read::<i16>(actor, 5078) > 0) else {
            PLATE.show(layout, None);
            return original!(widget);
        };
        let max = read::<f32>(widget, 124);
        if layout == 0 || !max.is_finite() || max <= 0. {
            return original!(widget);
        }
        let (phase, points, polish) = battle::hud_state(actor);
        let full = points == program::FLAME_MAX;
        let state = u32::from(phase) + if full && phase >= 2 { 2 } else { 0 };
        native!(0x6578f8, widget);
        write(widget, 116, max * points as f32 / program::FLAME_MAX as f32);
        write(widget, 132, if full { 5u32 } else { 1u32 });
        write(widget, 148, u8::from(full));
        native!(0x54e344, widget + 96);
        let result = native!(0x54e38c, widget + 96, layout);
        PLATE.show(layout, Some(5 * state + polish));
        result
    }
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), twin_sword_gauge);
}
