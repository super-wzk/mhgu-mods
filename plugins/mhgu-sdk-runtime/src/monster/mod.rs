mod records;

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod runtime;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub(super) use runtime::install;
