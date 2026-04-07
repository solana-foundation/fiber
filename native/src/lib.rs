//! Native Solana program implementation of the Fiber payment channel.
//!
//! Uses the `fastlane` macro for zero-overhead instruction dispatch with
//! compile-time account layout validation. This serves as the baseline
//! implementation for CU benchmarking against framework-based variants.

#![no_std]
#![cfg_attr(target_os = "solana", feature(asm_experimental_arch))]

use fiber::{nostd_panic_handler, prelude::*};

nostd_panic_handler!();

fastlane::fastlane! {
    // Fixed layout — compile-time offsets, zero parsing
    #[fixed(signer(0), writable(42))]
    IX_OPEN => Channel::open,

    #[fixed(signer(0), writable(42))]
    IX_FINALIZE => Channel::finalize,

    // Variable layout — pinocchio-optimized account parsing, parsed once
    #[variable]
    IX_DISTRIBUTE => Channel::distribute,

    #[variable(signer_check)]
    IX_BATCH_FINALIZE => Channel::batch_finalize,

    #[variable(signer_check)]
    IX_BATCH_OPEN => Channel::batch_open,

    #[variable]
    IX_BATCH_DISTRIBUTE => Channel::batch_distribute,

    #[variable]
    IX_DISTRIBUTE_TOKEN => Channel::distribute_token,
}
