#![cfg_attr(target_os = "solana", feature(asm_experimental_arch))]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod accounts;
mod channel;
pub mod cpi;
pub mod hash;
mod operator;
pub mod panic_handler;

/// Helper to read a value at offset and cast it
///
/// # Safety
/// - The caller must ensure that `ptr.add(offset)` is a valid pointer and properly aligned for type `T`.
/// - The memory at the computed address must be initialized and valid for reads of type `T`.
#[inline(always)]
const unsafe fn read<T>(ptr: *const u8, offset: usize) -> T
where
    T: core::marker::Copy,
{
    *ptr.add(offset).cast::<T>()
}

/// Helper to write a value at offset
///
/// # Safety
/// - The caller must ensure that `ptr.add(offset)` is a valid pointer and properly aligned for type `T`.
/// - The memory at the computed address must be valid for writes of type `T`.
#[inline(always)]
unsafe fn write<T>(ptr: *mut u8, offset: usize, value: T)
where
    T: core::marker::Copy,
{
    *ptr.add(offset).cast::<T>() = value;
}

/// Exit the program with a custom error code.
/// Written as a standalone extern "C" function so the SBF calling convention
/// places `code` in r1, then we move to r0 and exit.
/// `#[inline(never)]` prevents the optimizer from merging error paths.
#[cfg(target_os = "solana")]
#[inline(never)]
#[allow(clippy::missing_safety_doc)]
pub unsafe fn fail(code: u64) -> ! {
    // Compiler will have placed `code` on the stack or in a register.
    // We use a volatile read + lddw pattern. Since this is #[inline(never)],
    // each call site gets a unique call instruction, preventing merging.
    //
    // We can't use `in(reg)` with SBF, so we write code to a known
    // memory location and load it.
    let code_ptr = &code as *const u64;
    core::arch::asm!(
        "ldxdw r0, [{0}+0]",
        "exit",
        in(reg) code_ptr,
        options(noreturn)
    );
}

/// Stub for non-solana targets (tests compiled for host).
#[allow(dead_code)]
#[cfg(not(target_os = "solana"))]
#[allow(clippy::missing_safety_doc)]
pub unsafe fn fail(_code: u64) -> ! {
    unreachable!("fail() called in non-SBF context");
}

pub mod prelude {
    pub use crate::channel::{
        Channel, IX_BATCH_DISTRIBUTE, IX_BATCH_FINALIZE, IX_BATCH_OPEN, IX_DISCRIMINATOR,
        IX_DISTRIBUTE, IX_DISTRIBUTE_TOKEN, IX_FINALIZE, IX_OPEN,
    };
    pub use crate::operator::Operator;
    #[cfg(not(feature = "std"))]
    pub use crate::panic_handler::*;
}
