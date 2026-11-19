unsafe fn read<T: Copy>(address: u32, offset: u32) -> T {
    unsafe { ((address + offset) as *const T).read_unaligned() }
}

#[forge::hook(offset = 0xa956d8)]
unsafe fn draw_model(generator: u32, context: u32, gain: u32) -> u32 {
    const STATE_STACK_DEPTH_OFFSET: u32 = 0x308e4;
    const STATE_STACK_CAPACITY: u32 = 5;
    const FULL_STATE: u32 = 1;

    unsafe {
        if read::<u8>(generator, 70) != 5 {
            return original!(generator, context, gain);
        }
        let params: u32 = read(generator, 52);
        if params == 0
            || !matches!(
                &read::<[u8; 19]>(params, 80),
                b"effect\\frontier_gs\\" | b"effect\\frontier_db\\"
            )
        {
            return original!(generator, context, gain);
        }

        let depth = read::<u32>(context, STATE_STACK_DEPTH_OFFSET);
        if depth > STATE_STACK_CAPACITY {
            return 0;
        }
        if depth == STATE_STACK_CAPACITY {
            // GU 87A7AC/87B778 copy the live bindings at +0xAC. Mirror
            // their full-state copy off-stack, leaving every outer slot
            // and the command-list cursors owned by the caller.
            const LIVE_BINDINGS_OFFSET: u32 = 0xac;
            const FIXED_STATE_BYTES: usize = 344;
            const BINDING_BYTES: usize = 8;
            const MAX_BINDINGS: u32 = 4096;
            let binding_count = read::<u32>(context, 4);
            if binding_count > MAX_BINDINGS {
                return 0;
            }
            let length = FIXED_STATE_BYTES + BINDING_BYTES * binding_count as usize;
            let live = (context + LIVE_BINDINGS_OFFSET) as *mut core::mem::MaybeUninit<u8>;
            let mut snapshot = alloc::vec::Vec::<u8>::new();
            if snapshot.try_reserve_exact(length).is_err() {
                return 0;
            }
            let saved = snapshot.spare_capacity_mut().as_mut_ptr();
            core::ptr::copy_nonoverlapping(live, saved, length);
            let result = original!(generator, context, gain);
            core::ptr::copy_nonoverlapping(saved, live, length);
            return result;
        }

        // Preserve the bindings needed by later passes, including the stage
        // fog table. GU keeps the particle's draw packet in the command list.
        let base = forge::mem::text_addr();
        let save_state: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(base + 0x87a7ac);
        let restore_state: unsafe extern "C" fn(u32, u32) = core::mem::transmute(base + 0x87b778);
        save_state(context, FULL_STATE);
        let result = original!(generator, context, gain);
        restore_state(context, FULL_STATE);
        result
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, draw_model);
}
