# fastlane!

Zero-cost entrypoint macro for Solana programs that mixes compile-time hardcoded offsets with runtime account parsing.

## The Problem

Solana programs that handle both fixed-layout instructions (always 2 accounts, known sizes) and variable-layout instructions (N accounts, dynamic) face a tradeoff:

- **Hardcoded offsets** (19 CU) — fastest, but only works when you know the exact account layout at compile time
- **Account parsing** (50+ CU overhead) — flexible, but pays the parsing cost even for simple instructions

## The Solution

`fastlane!` lets you declare both in one macro. Fixed-layout instructions get compile-time offsets. Variable-layout instructions get Pinocchio's loop-unrolled deserializer. The dispatch is a single `num_accounts` check — zero overhead.

## Usage

```rust
#![no_std]

use fiber::{nostd_panic_handler, prelude::*};

nostd_panic_handler!();

fastlane::fastlane! {
    // Fixed layout — offsets computed at compile time from account data sizes.
    // signer(0) = 0 bytes of data, writable(42) = 42 bytes of data.
    // The macro computes: channel data starts at 0x28c0, ix data at 0x5100.
    #[fixed(signer(0), writable(42))]
    IX_OPEN => Channel::open,

    #[fixed(signer(0), writable(42))]
    IX_FINALIZE => Channel::finalize,

    // Variable layout — account walker parses at runtime.
    // Pinocchio's loop-unrolled deserializer handles the parsing.
    // Accounts and ix_offset are passed to the handler.
    #[variable]
    IX_DISTRIBUTE => Channel::distribute,

    // signer_check = verify accounts[0].is_signer()
    #[variable(signer_check)]
    IX_BATCH_FINALIZE => Channel::batch_finalize,
}
```

## What it generates

For `#[fixed(signer(0), writable(42))]`:
- Compile-time constants for every account's pubkey, lamports, and data offsets
- Compile-time constant for the instruction data offset
- Signer check at the hardcoded header offset (2 CU)
- No account parsing, no walker, no deserialization

For `#[variable]`:
- Pinocchio's `deserialize()` called once (loop-unrolled, MaybeUninit)
- `&mut [AccountView]` + `ix_offset` passed to the handler
- Optional `signer_check` flag for operator-signed batch instructions

The dispatch:
```rust
if num_accounts == FIXED_COUNT {
    // hardcoded path — 19 CU
} else {
    // pinocchio parser — 50+ CU overhead, but only paid once
}
```

## Handler signatures

Fixed-layout handlers receive the raw input pointer:
```rust
pub unsafe fn open(ptr: *mut u8) { ... }
```

Variable-layout handlers receive parsed accounts:
```rust
pub unsafe fn distribute(
    ptr: *mut u8,
    accounts: &mut [AccountView],
    num_accounts: usize,
    ix_offset: usize,
) { ... }
```

## Requirements

- `pinocchio` — for the variable-path account deserializer
- `solana-account-view` — for `AccountView` type
- `fiber` — for `fail()` error exit function (or provide your own)
