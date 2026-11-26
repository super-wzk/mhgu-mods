use mhgu_sdk::combat::collision::Contact;

#[forge::hook(offset = 0x1696fc)]
unsafe fn expand(hit: u32) -> u32 {
    if let Some(contact) = unsafe { Contact::from_raw(hit) } {
        let shapes = unsafe { crate::style::expand_world_contact(contact.owner_address(), hit) };
        if shapes != 0 {
            return shapes;
        }
    }
    original!(hit)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, expand);
}
