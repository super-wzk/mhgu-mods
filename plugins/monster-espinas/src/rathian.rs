//! Host class and projectile policy specific to this plugin.
use mhgu_sdk::{
    combat::shell::ShellLaunch,
    geometry::Angle,
    monster::{MonsterClass, MonsterUpdate, shell::DirectedShellResource},
};
// CEFE98 initializes the em001_00 vtable. D0CA90 chooses the resource ID;
// D0CC44 reads it from this slot into 3F93F8's setup for the 3F97EC shell.
pub(super) const RATHIAN: MonsterClass = MonsterClass::at_vtable(0x179acf8);
const PRIMARY_SHELL_RESOURCE_ID_OFFSET: u32 = 51908;

fn primary_shell(update: MonsterUpdate<'_>) -> Option<DirectedShellResource<'_>> {
    let actor = update.monster();
    if !RATHIAN.matches(actor) {
        return None;
    }
    // SAFETY: after the class check, D0CC44 reads this u32 as the resource for
    // the directed-shell setup. Resolve it each time; variants share the layout, not the ID.
    unsafe {
        let id = super::u32at(actor.as_raw(), PRIMARY_SHELL_RESOURCE_ID_OFFSET);
        DirectedShellResource::from_native_id(update, id)
    }
}

pub(super) fn fire(update: MonsterUpdate, origin: [f32; 3], pitch: Angle, yaw: Angle, speed: f32) {
    let Some(resource) = primary_shell(update) else {
        return;
    };
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let velocity = [
        sin_yaw * cos_pitch * speed,
        -sin_pitch * speed,
        cos_yaw * cos_pitch * speed,
    ];
    resource.spawn(ShellLaunch {
        origin,
        pitch,
        yaw,
        velocity,
    });
}
