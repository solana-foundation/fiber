# Fiber Payment Channel — Verification Spec v1.0

Fiber is a Solana payment channel program with a 3-state lifecycle (Open → Finalized → Closed) and a two-phase close (finalize + distribute).

## 0. Security Goals

1. **No double-distribute**: A channel MUST NOT be distributed more than once. Once status is `Closed`, no further state transitions are possible.
2. **No re-open**: A `Finalized` channel MUST NOT transition back to `Open`. A `Closed` channel MUST NOT transition to any other state.
3. **Lifecycle monotonicity**: Status transitions MUST be strictly monotonic: `Open(0) → Finalized(1) → Closed(2)`.

## 1. State Model

```
structure ChannelState where
  deposit : U64           -- total escrowed
  settled : U64           -- cumulative settled watermark
  status  : Status        -- Open | Finalized | Closed
```

```
inductive Status where
  | Open      -- 0
  | Finalized -- 1
  | Closed    -- 2
```

Lifecycle diagram:
```
  Open ──finalize──▶ Finalized ──distribute──▶ Closed
                                                  ╳
                                           (terminal state)
```

## 2. Operations

### 2.1 open
**Signers**: operator
**Preconditions**: `status = Open` AND `deposit = 0` (uninitialized)
**Effects**: set deposit, distribution_hash, status = Open
**Postconditions**: `deposit > 0`, `settled = 0`, `status = Open`

### 2.2 finalize
**Signers**: operator
**Preconditions**: `status = Open`
**Effects**: set `settled = new_settled`, `status = Finalized`
**Postconditions**: `settled > 0`, `settled <= deposit`, `status = Finalized`

### 2.3 distribute
**Signers**: none (permissionless)
**Preconditions**: `status = Finalized`
**Effects**: transfer lamports, set `status = Closed`
**Postconditions**: `status = Closed`

## 3. Formal Properties

### 3.1 Lifecycle Safety

**P1 (closed_is_terminal)**: For all channels `c` and transitions `t`,
if `c.status = Closed` then `t(c) = none`.

**P2 (finalized_cannot_reopen)**: For all channels `c` and transitions `t`,
if `c.status = Finalized` and `t(c) = some c'` then `c'.status ≠ Open`.

**P3 (status_monotonic)**: For all channels `c` and transitions `t`,
if `t(c) = some c'` then `c'.status.toNat ≥ c.status.toNat`.

### 3.2 No Double Distribute

**P4 (distribute_once)**: For all channels `c`,
if `distribute(c) = some c'` then `c'.status = Closed`
and `distribute(c') = none`.

## 4. Trust Boundary

The following are axiomatic (not verified):
- **Account ownership**: The Solana runtime ensures only the Fiber program can modify channel account data. We assume the runtime is correct.
- **Operator honesty**: The operator chooses which voucher to settle. We do not verify that the latest voucher is used — only that the on-chain state machine is correct.
- **Hash correctness**: The distribution hash is computed correctly by the client at open time. We verify that distribute checks the hash, not that the hash was computed correctly.

## 5. Verification Results

| Property | Status | Proof |
|---|---|---|
| P1 (closed_is_terminal) | **Proved** | `Proofs/Lifecycle.lean` |
| P2 (finalized_cannot_reopen) | **Proved** | `Proofs/Lifecycle.lean` |
| P3 (status_monotonic) | **Proved** | `Proofs/Lifecycle.lean` |
| P4 (distribute_once) | **Proved** | `Proofs/Lifecycle.lean` |
