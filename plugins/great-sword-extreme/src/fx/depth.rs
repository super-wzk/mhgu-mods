//! Supply the animated camera bias to GU's own model-particle transform.
use super::field;

const LOAD_BIAS: u32 = 0xa957cc;

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global extreme_model_depth",
    ".hidden extreme_model_depth",
    ".hidden extreme_particle_bias",
    ".type extreme_model_depth,%function",
    "extreme_model_depth:",
    "push {r0-r3,r12,lr}",
    "vpush {d0-d7}",
    "mov r0,r5",
    "bl extreme_particle_bias",
    "str r0,[sp,#8]",  // saved s2
    "str r1,[sp,#80]", // saved ip
    "vpop {d0-d7}",
    "pop {r0-r3,r12,lr}",
    "ldr r2,[r1,#0x100]",
    "bx ip",
    options(raw),
);

unsafe extern "C" {
    fn extreme_model_depth();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn extreme_particle_bias(generator: u32, params: u32) -> u64 {
    unsafe {
        let mut bias = field::<f32>(params, 272);
        if field::<[u8; 4]>(params, 128) == *b"FZD1" {
            let particle: u32 = field(generator, 176);
            let curve = params + field::<u32>(params, 132);
            let count: u32 = field(curve, 0);
            if particle != 0 && count >= 2 {
                let age: u32 = field(particle, 20);
                let [mut start, value]: [u32; 2] = field(curve, 4);
                bias = f32::from_bits(value);
                for i in 1..count {
                    let [end, next]: [u32; 2] = field(curve, 4 + 8 * i);
                    if age < end {
                        let fraction = age.saturating_sub(start) as f32 / (end - start) as f32;
                        bias += (f32::from_bits(next) - bias) * fraction;
                        break;
                    }
                    start = end;
                    bias = f32::from_bits(next);
                }
            }
        }
        (u64::from(forge::mem::text_addr() + LOAD_BIAS + 8) << 32) | u64::from(bias.to_bits())
    }
}

pub(super) fn install() {
    unsafe {
        let target = forge::mem::text_addr() + LOAD_BIAS;
        // Only replace GU 1.4's vldr s2 and ldr r2. The shim restores
        // the live registers and returns to the following instruction.
        if field::<[u32; 2]>(target, 0) == [0xed911a44, 0xe5912100] {
            let mut original = core::ptr::null();
            forge::sys::hook::forge_hook_create(
                target as *const _,
                extreme_model_depth as *const _,
                &mut original,
            );
        }
    }
}
