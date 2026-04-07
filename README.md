# Fiber

> [!IMPORTANT]
> This is a research prototype — not production code. The programs are unaudited, use extensive `unsafe` Rust, and have not been tested with real tokens. Do not deploy or use with real funds.

Research into ultra-optimized payment channels for Solana, targeting the `session` intent of the [MPP specification](https://github.com/solana-foundation/mpp-specs).

## Why Payment Channels

The [MPP SDK](https://github.com/solana-foundation/mpp-sdk) defines two intents for HTTP-authenticated payments: `charge` (one-time) and `session` (metered/streaming). Charges work well for discrete purchases, but most real-world payment flows are continuous:

- **AI inference**: per-token or per-request billing across a conversation
- **API metering**: pay-per-call with variable cost per endpoint
- **Streaming media**: per-second or per-byte delivery
- **Data feeds**: continuous price/market data subscriptions

These services bill per-request, but settling each request as a separate on-chain transaction doesn't make economic sense at high frequency. Payment channels solve this: open a channel once, deposit funds into escrow, authorize spending via off-chain signed vouchers (one per request, zero on-chain cost), settle once at close. Thousands of requests, three on-chain transactions.

### Operator Efficiency

Solana has plenty of compute capacity. The bottleneck isn't the chain — it's how many channels a **single operator** can settle per transaction. Each transaction costs a base fee + priority fee regardless of how much work it does. An operator settling 1 channel per transaction pays the same base fee as one settling 24.

Lower CU per channel and higher batch density mean:
- Fewer transactions to settle the same number of channels
- Lower total fees for the operator
- Faster settlement during traffic spikes (more channels per slot)

This repo explores how to minimize per-channel cost and maximize batch density from the operator's perspective.

## Approach

Fiber explores three ideas:

### 1. Two-Phase Close (finalize + distribute)

Instead of a single atomic close instruction, split it:

- **Finalize** — advances the settled watermark and marks the channel as done. Lightweight (~19 CU), no token transfers. This is the time-sensitive operation (protects against forced close).
- **Distribute** — transfers tokens to recipient, split recipients, and refunds payer. Heavier but permissionless — anyone can crank it anytime.

This separation lets operators finalize hundreds of channels quickly, then distribute in separate transactions at leisure.

### 2. Distribution Hash

Instead of storing the full distribution config on-chain (recipient + up to 8 splits = 378 bytes), store a **16-byte truncated Blake3 hash** of the config. Total channel account: **42 bytes**. At distribute time, the caller provides the full config and the program verifies it against the hash.

Tradeoff: pushes ~350 bytes from storage (expensive, permanent) to instruction data (cheap, transient).

### 3. Batch Instructions

Single instructions that process N channels, amortizing per-instruction framing overhead (~21 bytes per instruction eliminated). The value depends on the ratio of framing to payload:

- **BatchFinalize** — per-channel payload is 8 bytes (settled amount), so framing is 72% of per-channel cost. Eliminating it roughly doubles density.
- **BatchOpen** — per-channel payload is 24 bytes (deposit + hash), so framing is 47% — modest density gain. The real value is **composability with [multi-delegator](https://github.com/solana-foundation/multi-delegator)**: payers pre-authorize deposits, and the operator opens channels on their behalf as sole signer.
- **BatchDistribute** — permissionless batch distribution of finalized channels.

### Instruction Set

| Instruction | Accounts | Signer | Description |
|---|---|---|---|
| `open` | operator, channel | operator | Initialize channel with deposit + distribution hash |
| `finalize` | operator, channel | operator | Advance settled watermark |
| `distribute` | channel, recipient, payer, splits... | none | Verify hash, transfer lamports, close channel |
| `distribute_token` | channel, escrow, authority, token_program, recipient, payer, splits... | none (PDA) | Same but via CPI to token program |
| `batch_open` | operator, channels... | operator | Open N channels in one instruction |
| `batch_finalize` | operator, channels... | operator | Finalize N channels in one instruction |
| `batch_distribute` | (channel, recipient, payer, splits...)... | none | Distribute N channels in one instruction |

## Benchmarks

### Framework Comparison

Same program logic (open, finalize, distribute with Blake3 hash verification) implemented in four frameworks. All benchmarks use correctly-sized buffers — see "The Eternal Framework Debate" below.

| Instruction | Native + fastlane! | Pinocchio | Quasar | Anchor |
|---|---|---|---|---|
| **Open** | **19** | 54 | 50 | 1,937 |
| **Finalize** | **19** | 50 | **37** | 1,922 |
| **Distribute (0 splits)** | 255 | **210** | 237 | 2,587 |
| **Distribute (2 splits)** | 425 | **397** | 589 | 3,283 |
| **Distribute (16 splits)** | **1,531** | 1,558 | 3,459 | 7,749 |
| **Distribute (32 splits)** | **2,787** | 2,879 | 8,179 | 12,853 |

Native + `fastlane!` uses compile-time hardcoded offsets for fixed-layout instructions (open/finalize) and Pinocchio's loop-unrolled deserializer for variable-account instructions. 2.8x faster than Pinocchio on open, within 17% on small distributes, fastest at 16+ splits.

#### Per-Split Marginal Cost

| Framework | CU/split |
|---|---|
| **Native + fastlane!** | **~79** |
| Pinocchio | ~83 |
| Quasar | ~248 |
| Anchor | ~331 |

### Batch Benchmarks

#### Single-Channel vs Batch

| Instruction | CU | CU/channel |
|---|---|---|
| Open (single) | 19 | 19 |
| Finalize (single) | 19 | 19 |
| BatchOpen (63 ch) | 1,858 | **29** |
| BatchFinalize (63 ch) | 1,733 | **28** |
| BatchDistribute (3 ch, 0 splits) | 776 | 259 |

#### Native vs Pinocchio Batch

Both implementing the same batch logic:

| Instruction | Native + fastlane! | Pinocchio | Advantage |
|---|---|---|---|
| BatchFinalize (63 ch) | **1,733** | 2,036 | **15%** |
| BatchOpen (63 ch) | **1,858** | 2,793 | **34%** |

`fastlane!`'s compile-time offset path skips account parsing for the operator, giving a structural advantage on batch instructions.

### Batch Density

Transaction size is the bottleneck, not CU.

**ALT considerations**: Address Lookup Tables reduce per-account cost from 32 bytes to 1 byte, but require a 1-slot warm-up (~400ms). Practical for **distribute** (not time-sensitive, stable addresses) but not for **finalize** or **open** (time-sensitive, new addresses).

#### Finalize (no ALT)

| Approach | 1,232B tx | 4,928B tx |
|---|---|---|
| Separate instructions | ~16 | ~77 |
| **Batch instruction** | **~24** | **~116** |

#### Open (no ALT)

| Approach | 1,232B tx | 4,928B tx |
|---|---|---|
| Separate instructions | ~13 | ~61 |
| **Batch instruction** | **~18** | **~84** |

#### Distribute (2 splits, ALT)

| Approach | 1,232B tx | 4,928B tx |
|---|---|---|
| Separate instructions | ~16 | ~76 |
| **Batch instruction** | **~33** | **~155** |

### Operator Throughput

At 1 transaction per slot (400ms):

| Operation | ALT? | Channels/tx | Per second | Per day |
|---|---|---|---|---|
| BatchFinalize | no | 24 | 60 | 5.2M |
| BatchOpen | no | 18 | 45 | 3.9M |
| BatchDistribute (2 splits) | yes | 33 | 82 | 7.1M |

### SPL Token Cost Projections

Distribute currently uses direct lamport transfers. For USDC channels, each transfer requires CPI into the token program:

| Token Program | CU/transfer | Distribute (2 splits, 4 transfers) |
|---|---|---|
| SPL Token | ~5,000 | ~20,600 |
| **p-token** | **~78** | **~2,500** |

p-token CU costs measured on [testnet](https://explorer.solana.com/tx/dXdSNigy6c5NqeihQ9nr15AcuoRR11NP6P3YpW2bM36CPKgeDErxsqnkJ5M9RVKg2QJcb3grxSspfdwju5SJVs8?cluster=testnet).

### Reference: Existing Payment Channel (SPL Token)

For comparison, a recently benchmarked Solana payment channel using standard SPL Token CPI:

| Instruction | CU |
|---|---|
| create_escrow | 7,811 |
| deposit (new vault) | 23,023 |
| deposit (existing vault) | 14,789 |
| register_session_key | 10,919 |
| submit_authorization (1 split) | 18,583 |
| finalize (1 split) | 18,039 |
| close_escrow (1 mint) | 19,858 |

## Building

```bash
# Build the SBF programs
cargo build-sbf --manifest-path native/Cargo.toml
cargo build-sbf --manifest-path frameworks/pinocchio/Cargo.toml
cargo build-sbf --manifest-path frameworks/quasar/Cargo.toml
cargo build-sbf --manifest-path frameworks/anchor/Cargo.toml

# Run tests
cargo test -p fiber-native --test tests

# Run CU benchmarks
cargo bench -p fiber-native
cargo bench -p pinocchio
cargo bench -p quasar
cargo bench -p anchor
```

## The Eternal Framework Debate

The benchmark numbers tell a clear story on performance:

| Framework | 32-split distribute | Relative |
|---|---|---|
| **Native + fastlane!** | **2,787 CU** | **1x** |
| Pinocchio | 2,879 CU | 1.03x |
| Quasar | 6,514 CU | 2.3x |
| Anchor | 12,853 CU | 4.6x |

But they don't tell the full story.

### What you give up for performance

To achieve 19 CU on open and 28 CU/channel on batch finalize, Fiber's native implementation:

- **Reads account data via raw pointer arithmetic** — `*(ptr.add(0x28c0) as *const u64)`. One wrong offset and you're reading garbage. No compiler error, no runtime error, just wrong results or silent corruption.
- **Manages lamports via transmute hacks** — casting `&AccountView` through raw pointers to mutate lamports without `&mut`. Sound in practice (interior raw pointer), UB by the Rust reference.
- **Computes memory layouts at compile time** — the `fastlane!` macro calculates that "with a 0-byte account followed by a 42-byte account, instruction data starts at offset 0x5100." Change the account data size and every hardcoded offset breaks silently.
- **Uses `MaybeUninit` stack buffers with manually computed sizes** — get the size wrong and you corrupt the stack (see below).

This is not Rust in any meaningful sense. It's C with Rust syntax. You lose the borrow checker, the type system, bounds checking — everything that makes Rust worth using.

### A real bug we found

During benchmarking, we measured Pinocchio's distribute at **579 CU for 16 splits** — suspiciously fast. The cause: a stack buffer overflow. The hash input buffer was `[u8; 352]` (sized for 8 splits), but 16 splits write 672 bytes. SBF has no stack guards, so the write silently corrupted adjacent memory. The hash "verified" because both the stored hash and the runtime computation used the same corrupted buffer.

After fixing the buffer to `[u8; 672]`, the cost jumped to **1,558 CU**. The "fast" benchmark was measuring a broken program.

This class of bug is **impossible in Anchor** — it uses heap-allocated `Vec` with bounds checking. It's possible in every `unsafe` framework: Pinocchio, Quasar, and raw Fiber.

| Framework | Buffer strategy | Buffer overflow risk |
|---|---|---|
| Anchor | Heap `Vec` (bounds-checked) | **None** |
| Native + fastlane! | Stack `MaybeUninit` (manually sized) | Programmer error |
| Pinocchio | Stack `MaybeUninit` (manually sized) | Programmer error |
| Quasar | Stack `MaybeUninit` (manually sized) | Programmer error |

### The tradeoff

Anchor is 4.6x slower but catches this entire category of bugs at compile time.

For this research repo, we went full `unsafe` to find the floor. The floor is ~19 CU for finalize and ~79 CU per split.

## Future Work

- **SPL token CPI distribute**: implement distribute with CPI into p-token for USDC channels.
- **Multi-delegator composition**: compose batch_open with [multi-delegator](https://github.com/solana-foundation/multi-delegator) for operator-driven batch opens from pre-authorized payer deposits.
- **P256/passkey batch open**: payers sign off-chain authorizations with passkeys, operator collects and submits batch.
- **Extract `fastlane!`**: the zero-cost entrypoint macro is framework-agnostic — computes memory offsets at compile time for fixed-layout instructions, falls back to Pinocchio's deserializer for variable layouts.

## Credits

- [Pinocchio](https://github.com/anza-xyz/pinocchio) — loop-unrolled account deserializer used in `fastlane!`
- [Doppler](https://github.com/blueshift-gg/doppler) — `no_std` raw pointer patterns
- [Quasar](https://github.com/blueshift-gg/quasar) — zero-copy Solana framework
- [Multi-Delegator](https://github.com/solana-foundation/multi-delegator) — managed token delegations
