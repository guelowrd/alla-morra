//! Minimal no_std stub of the wasi v0.11 crate for Miden contract compilation.
//!
//! The real wasi crate uses `extern crate std;` which conflicts with
//! cargo-miden's -Z build-std when targeting wasm32-wasip2.

#![no_std]

pub type Size = usize;

#[repr(transparent)]
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Errno(pub u16);

impl Errno {
    #[inline(always)]
    pub fn raw(self) -> u16 {
        self.0
    }
}

/// Stub: Miden contracts never call this at runtime.
#[inline(always)]
pub unsafe fn random_get(_buf: *mut u8, _buf_len: Size) -> Result<(), Errno> {
    // In the Miden VM, contracts are deterministic. This function is never
    // called. Returning success with zeroed bytes would be fine, but since
    // the contracts don't call getrandom at runtime, we just return an error.
    Err(Errno(1))
}
