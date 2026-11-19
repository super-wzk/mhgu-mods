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

        if read::<u32>(context, STATE_STACK_DEPTH_OFFSET) >= STATE_STACK_CAPACITY {
            return 0;
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
