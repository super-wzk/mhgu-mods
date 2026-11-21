//! Adapt Frontier model-particle draws to GU 1.4.

use core::ffi::c_void;
use mhgu_sdk::style::particle::{self, DrawModel, ModelRenderer};

const SUBMIT: u32 = 0xc90380;
const MODEL_PATH: u32 = 80;
const MODEL_DRAW_RETURNS: [u32; 4] = [0xa95d10, 0xa96458, 0xa96b28, 0xa96fa8];
// FZ HD pixel shader 0x37 encodes linear depth as (view_depth - 7) / 4993.
const FZ_DEPTH_RANGE: f32 = 5000. - 7.;
static mut GAME_TEXT: u32 = 0;
static mut ORIGINAL_SUBMIT: *const c_void = core::ptr::null();

unsafe fn read<T: Copy>(address: u32, offset: u32) -> T {
    unsafe { ((address + offset) as *const T).read_unaligned() }
}

unsafe extern "C" fn matches(params: u32) -> bool {
    params != 0
        && unsafe {
            matches!(
                &read::<[u8; 19]>(params, MODEL_PATH),
                b"effect\\frontier_gs\\"
                    | b"effect\\frontier_db\\"
                    | b"effect\\frontier_ls\\"
                    | b"effect\\frontier_sa\\"
            )
        }
}

// C90380 takes the first four words in r0-r3, six more on the stack,
// and a specular multiplier in s0.
#[repr(C)]
struct DrawArgs {
    effect: u32,
    context: u32,
    mesh: u32,
    transform: u32,
    color: u32,
    light_gain: u32,
    particle: u32,
    extra_pass: u32,
    uv: u32,
    mode: u32,
}

const _: () =
    assert!(core::mem::offset_of!(DrawArgs, color) == 16 && core::mem::size_of::<DrawArgs>() == 40);

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global frontier_model_submit",
    ".hidden frontier_model_submit",
    ".hidden frontier_model_packet",
    ".type frontier_model_submit,%function",
    "frontier_model_submit:",
    "push {r0-r4,lr}",
    "mov r0,sp",
    "mov r1,lr",
    "vmov r2,s0",
    "bl frontier_model_packet",
    "add sp,sp,#20",
    "pop {pc}",
    ".global frontier_call_model",
    ".hidden frontier_call_model",
    ".type frontier_call_model,%function",
    "frontier_call_model:",
    "push {r4-r6,lr}",
    "sub sp,sp,#24",
    "mov r6,r0",
    "mov r5,r1",
    "vmov s0,r2",
    "add ip,r6,#16",
    "ldmia ip!,{r0-r3}",
    "stmia sp,{r0-r3}",
    "ldmia ip,{r0,r1}",
    "strd r0,r1,[sp,#16]",
    "ldmia r6,{r0-r3}",
    "blx r5",
    "add sp,sp,#24",
    "pop {r4-r6,pc}",
    options(raw),
);

unsafe extern "C" {
    fn frontier_model_submit();
    fn frontier_call_model(args: *const DrawArgs, original: *const c_void, specular: u32) -> u32;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn frontier_model_packet(
    saved: *const u32,
    return_address: u32,
    specular: u32,
) -> u32 {
    unsafe {
        // The entry stub pushes r0-r4 and lr below the original stack.
        let stack = saved.add(6);
        let [effect, context, mesh, transform] = saved.cast::<[u32; 4]>().read();
        let [color, light_gain, particle, extra_pass, uv, mode] = stack.cast::<[u32; 6]>().read();
        let mut args = DrawArgs {
            effect,
            context,
            mesh,
            transform,
            color,
            light_gain,
            particle,
            extra_pass,
            uv,
            mode,
        };
        let submit = |args: &DrawArgs| frontier_call_model(args, ORIGINAL_SUBMIT, specular);
        let base = GAME_TEXT;
        if !MODEL_DRAW_RETURNS.contains(&return_address.wrapping_sub(base)) {
            return submit(&args);
        }
        // A956D8 saves the generator at sp+0x44 in all four submit branches.
        let generator = *stack.add(0x44 / 4);
        let params: u32 = read(generator, 52);
        if !matches(params) {
            return submit(&args);
        }

        let main_pass = read::<u32>(context, 360) & 0x2080000 == 0;
        let blend = read::<u16>(args.particle, 0) >> 5;
        let color: u32 = read(args.color, 0);
        let color = if main_pass && matches!(blend, 0 | 1) {
            // Match the 0.7 RGB gain in FZ HD's particle shader (0x37).
            // B007FC maps these selectors to additive and alpha blending.
            let [r, g, b, alpha] = color.to_le_bytes();
            let scale = |channel: u8| ((7 * u16::from(channel) + 5) / 10) as u8;
            u32::from_le_bytes([scale(r), scale(g), scale(b), alpha])
        } else {
            color
        };
        args.color = (&raw const color) as u32;

        if read::<[u8; 3]>(params, 136) != *b"FZF" {
            return submit(&args);
        }
        let mut matrix: [[f32; 4]; 4] = read(args.transform, 0);
        args.transform = (&raw const matrix) as u32;
        let axes: u8 = read(params, 139);
        for (axis, basis) in matrix[..2].iter_mut().enumerate() {
            // C90380 receives the particle at +24. Reuse the fixed scale
            // samples A9958C stores at particle +272 and +276.
            if axes & (1 << axis) != 0 && read::<f32>(args.particle, 248 + 4 * axis as u32) >= 0.5 {
                for component in basis {
                    // MHGURecomp's plugin interpreter rejects NEON VNEG.F32.
                    let mut bits = component.to_bits();
                    core::arch::asm!(
                        "eor {bits}, {bits}, #0x80000000",
                        bits = inout(reg) bits,
                        options(pure, nomem, nostack, preserves_flags),
                    );
                    *component = f32::from_bits(bits);
                }
            }
        }

        let threshold: f32 = read(params, 140);
        let alpha = color >> 24;
        if !(threshold > 0. && threshold.is_finite())
            || alpha == 0
            || !main_pass
            || read::<u32>(context, 0xd7c) & !15 == 0
        {
            return submit(&args);
        }

        let view: unsafe extern "C" fn(u32) -> *const [[f32; 4]; 4] =
            core::mem::transmute(base + 0x885804);
        let camera: unsafe extern "C" fn(u32) -> *const [[f32; 4]; 4] =
            core::mem::transmute(base + 0x885824);
        let view = &*view(context);
        let eye = (*camera(context))[3];
        let depth = -(matrix[3][0] * view[0][2]
            + matrix[3][1] * view[1][2]
            + matrix[3][2] * view[2][2]
            + view[3][2]);
        if !depth.is_finite() || depth < 1. {
            return submit(&args);
        }

        let fade_scale = (threshold * FZ_DEPTH_RANGE) / depth;
        if !fade_scale.is_finite() {
            return submit(&args);
        }
        let mut flags: [u32; 2] = read(args.particle, 0);
        // C90B50 selects DSZTest (443) from these flags for translucent
        // submissions. Both samples test the scene's depth without writing it.
        flags[1] &= !0x18;
        args.particle = flags.as_ptr() as u32;
        let original = matrix;
        let near_alpha = alpha / 2;
        let mut result = 0;
        for (fraction, part) in [(0.25, near_alpha), (0.75, alpha - near_alpha)] {
            if part == 0 {
                continue;
            }
            // Test at the midpoints of the FZ fade range. Scaling about
            // the eye keeps the projected footprint the same for both samples.
            let scale = 1. + fraction * fade_scale;
            for (basis, old) in matrix[..3].iter_mut().zip(&original[..3]) {
                for (component, value) in basis[..3].iter_mut().zip(old) {
                    *component = *value * scale;
                }
            }
            for axis in 0..3 {
                matrix[3][axis] = eye[axis] + (original[3][axis] - eye[axis]) * scale;
            }
            let color = (color & 0x00ff_ffff) | (part << 24);
            args.color = (&raw const color) as u32;
            result = submit(&args);
        }
        result
    }
}

unsafe extern "C" fn draw(
    native: DrawModel,
    generator: u32,
    params: u32,
    context: u32,
    gain: u32,
) -> u32 {
    unsafe {
        crate::trail::draw_bound(generator, (params + MODEL_PATH) as *const u8, || {
            native(generator, context, gain)
        })
    }
}

/// Install the shared provider from a weapon plugin's Forge entry.
pub fn install() {
    static RENDERER: ModelRenderer = ModelRenderer { matches, draw };
    if !mhgu_sdk::service::on_game_thread() || !unsafe { crate::trail::install() } {
        return;
    }
    if !particle::bind_model(&RENDERER) {
        return;
    }
    unsafe {
        let base = forge_sys::mem::forge_mem_getMainTextAddr();
        GAME_TEXT = base;
        forge_sys::hook::forge_hook_create(
            (base + SUBMIT) as *const _,
            frontier_model_submit as *const _,
            &raw mut ORIGINAL_SUBMIT,
        );
    }
}
