//! Bind the equipment's rModel at GU's weapon-part construction boundary.
use super::{guild, inventory, local_hunter, profile, resource, set};
use core::sync::atomic::{AtomicU16, Ordering};
use mhgu_sdk::weapon::EquipmentModel;

static PREVIEW: AtomicU16 = AtomicU16::new(0);

pub(super) fn preview(equipment: u16) {
    PREVIEW.store(equipment, Ordering::Relaxed);
}

unsafe fn read(addr: u32) -> u32 {
    unsafe { (addr as *const u32).read_unaligned() }
}

pub(super) fn mesh(model: EquipmentModel) -> Option<u32> {
    resource::load(0x2124df0, model.archive)?;
    resource::load(0x21251c8, model.model)
}

#[forge::hook(offset = 0x303790)]
unsafe fn update_preview_weapon(part: u32) -> u32 {
    unsafe {
        let actor = read(part + 48);
        let manager = read(forge::mem::text_addr() + 0x1885b58);
        if actor == 0
            || manager == 0
            || read(actor + 5060) != manager + 13884
            || read(actor + 9120) != part
            || guild::preview_equipment(manager, actor)
                .is_none_or(|weapon| weapon.definition.model.is_none())
        {
            return original!(part);
        }
        // GU skips the part update while a card mannequin's pose gate is
        // closed. Use the normal part update for the registered primary mesh.
        let flags = read(actor + 5068);
        set(actor, 5068, flags & !0x10);
        let out = original!(part);
        set(actor, 5068, read(actor + 5068) | (flags & 0x10));
        out
    }
}

#[forge::hook(offset = 0x307f18)]
unsafe fn bind(part: u32, args: *const [u32; 10]) -> u32 {
    unsafe {
        let manager = read(forge::mem::text_addr() + 0x1885b58);
        let actor = read(part + 48);
        if manager == 0 || actor == 0 || args.is_null() || !mhgu_sdk::service::on_game_thread() {
            return original!(part, args);
        }
        let equipment = read(actor + 5060);
        let definition = if equipment == manager + 13884 {
            guild::preview_equipment(manager, actor).map(|weapon| weapon.definition)
        } else if equipment == manager + 12508 || actor == read(manager + 44) {
            inventory::definition(PREVIEW.load(Ordering::Relaxed)).map(|(_, e)| e)
        } else if equipment == manager + 124 || actor == local_hunter() {
            inventory::selected()
                .and_then(|(item, _)| inventory::definition(item.equipment.get()).map(|(_, e)| e))
        } else {
            profile::peer_model(actor, equipment).map(|weapon| weapon.definition)
        };
        let Some(model) = definition.and_then(|e| e.model).and_then(mesh) else {
            return original!(part, args);
        };
        // 289670 builds ten words for this call. 939DA4 (virtual +88)
        // retains the mesh and rebinds the skeleton. The single model owns
        // all its moving parts, with no retail secondary mesh or weapon LMT.
        let mut binding = *args;
        binding[4] = if binding[0] == 0 { model } else { 0 };
        binding[5..].fill(0);
        original!(part, &binding)
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, bind);
    forge::install_hook!(base, update_preview_weapon);
}
