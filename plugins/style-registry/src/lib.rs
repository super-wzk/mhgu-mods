#![cfg_attr(target_arch = "arm", no_std)]
extern crate alloc;
#[cfg(any(target_arch = "arm", test))]
mod config;

#[cfg(target_arch = "arm")]
mod runtime;
