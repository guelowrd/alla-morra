//! Minimal no_std stub of the wasip2 crate for Miden contract compilation.
//!
//! The real wasip2 crate uses `extern crate std;` which conflicts with
//! cargo-miden's -Z build-std when targeting wasm32-wasip2. This stub provides
//! only the symbols that getrandom v0.3.4 needs, without any std dependency.

#![no_std]

pub mod random {
    pub mod random {
        /// Stub: Miden contracts are deterministic and never call this at runtime.
        #[inline(always)]
        pub fn get_random_u64() -> u64 {
            // In the Miden VM, contracts are deterministic. getrandom is only
            // in the dep tree for compilation; it is never called at runtime.
            // If it is called, the VM will trap (acceptable for a no_std stub).
            core::hint::black_box(0u64)
        }
    }
}
