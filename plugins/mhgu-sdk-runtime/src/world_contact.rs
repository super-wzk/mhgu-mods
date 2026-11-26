use alloc::boxed::Box;
use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use mhgu_sdk::combat::collision::{Contact, world};

struct Handler {
    expand: world::Expand,
    next: *mut Handler,
}

static HANDLERS: AtomicPtr<Handler> = AtomicPtr::new(ptr::null_mut());

extern "C" fn bind(expand: world::Expand) {
    // The SDK serializes registration on the game thread. Published nodes stay
    // immutable and resident, so GU can traverse them without holding a lock.
    let next = HANDLERS.load(Ordering::Relaxed);
    let handler = Box::leak(Box::new(Handler { expand, next }));
    HANDLERS.store(handler, Ordering::Release);
}

#[forge::hook(offset = 0x1696fc)]
unsafe fn expand(hit: u32) -> u32 {
    if let Some(contact) = unsafe { Contact::from_raw(hit) } {
        let shapes = unsafe { crate::style::expand_world_contact(contact.owner_address(), hit) };
        if shapes != 0 {
            return shapes;
        }
        let mut handler = HANDLERS.load(Ordering::Acquire);
        while let Some(current) = unsafe { handler.as_ref() } {
            let shapes = unsafe { (current.expand)(hit) };
            if shapes != world::PASS {
                return shapes;
            }
            handler = current.next;
        }
    }
    original!(hit)
}

pub(super) fn install(base: u32) {
    static API: world::Service = world::Service(bind);
    if unsafe { world::publish(&API) } {
        forge::install_hook!(base, expand);
    }
}
