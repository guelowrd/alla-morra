//! Stub getrandom for Miden contract compilation.
//!
//! Miden contracts are no_std and deterministic. They do NOT call getrandom at
//! runtime. This stub satisfies the transitive dependency (miden-crypto → rand →
//! rand_core → getrandom) without pulling in the `wasip2` crate, which causes a
//! duplicate `core` lang-item error when cargo-miden uses -Z build-std.

#![no_std]

use core::mem::MaybeUninit;

/// Error type matching the real getrandom API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(u32);

impl Error {
    pub const UNEXPECTED: Error = Error(0x80000000);
    pub const UNSUPPORTED: Error = Error(0x80000001);

    pub fn raw_os_error(self) -> Option<i32> {
        None
    }
}

#[cfg(feature = "std")]
mod error_std_impls {
    use super::Error;
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "getrandom error: {}", self.0)
        }
    }
    impl std::error::Error for Error {}
}

/// Fill `dest` with random bytes. Always errors in this stub.
#[inline]
pub fn fill(dest: &mut [MaybeUninit<u8>]) -> Result<&mut [u8], Error> {
    let _ = dest;
    Err(Error::UNSUPPORTED)
}

/// Get a random u32. Always errors in this stub.
#[inline]
pub fn u32() -> Result<u32, Error> {
    Err(Error::UNSUPPORTED)
}

/// Get a random u64. Always errors in this stub.
#[inline]
pub fn u64() -> Result<u64, Error> {
    Err(Error::UNSUPPORTED)
}
