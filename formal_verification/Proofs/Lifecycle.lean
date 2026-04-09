/-!
# Fiber Payment Channel — Lifecycle Safety Proofs

We model the channel state machine and prove:
- P1: Closed is terminal (no transitions from Closed)
- P2: Finalized cannot reopen
- P3: Status is monotonic
- P4: Distribute is one-shot
-/

inductive Status where
  | Open
  | Finalized
  | Closed
  deriving DecidableEq, Repr

def Status.toNat : Status → Nat
  | .Open => 0
  | .Finalized => 1
  | .Closed => 2

structure ChannelState where
  deposit : Nat
  settled : Nat
  status  : Status

def finalizeChannel (s : ChannelState) : Option ChannelState :=
  match s.status with
  | .Open => some { s with status := .Finalized }
  | _ => none

def distributeChannel (s : ChannelState) : Option ChannelState :=
  match s.status with
  | .Finalized => some { s with status := .Closed }
  | _ => none

-- P1: Closed is terminal

theorem finalize_rejects_closed (s : ChannelState)
    (h : s.status = .Closed) :
    finalizeChannel s = none := by
  simp [finalizeChannel, h]

theorem distribute_rejects_closed (s : ChannelState)
    (h : s.status = .Closed) :
    distributeChannel s = none := by
  simp [distributeChannel, h]

theorem closed_is_terminal (s : ChannelState) (h : s.status = .Closed) :
    finalizeChannel s = none ∧ distributeChannel s = none :=
  ⟨finalize_rejects_closed s h, distribute_rejects_closed s h⟩

-- P2: Finalized cannot reopen

theorem finalize_requires_open (s s' : ChannelState)
    (h : finalizeChannel s = some s') : s.status = .Open := by
  simp [finalizeChannel] at h; split at h <;> simp_all

theorem finalize_produces_finalized (s s' : ChannelState)
    (h : finalizeChannel s = some s') : s'.status = .Finalized := by
  simp [finalizeChannel] at h; split at h <;> first | contradiction | (obtain ⟨_, rfl⟩ := h; rfl)

theorem distribute_requires_finalized (s s' : ChannelState)
    (h : distributeChannel s = some s') : s.status = .Finalized := by
  simp [distributeChannel] at h; split at h <;> simp_all

theorem distribute_produces_closed (s s' : ChannelState)
    (h : distributeChannel s = some s') : s'.status = .Closed := by
  simp [distributeChannel] at h; split at h <;> first | contradiction | (obtain ⟨_, rfl⟩ := h; rfl)

theorem finalized_cannot_reopen (s : ChannelState) (hfin : s.status = .Finalized) :
    finalizeChannel s = none ∧
    (∀ s', distributeChannel s = some s' → s'.status = .Closed) := by
  constructor
  · simp [finalizeChannel, hfin]
  · intro s' h; exact distribute_produces_closed s s' h

-- P3: Status monotonicity

theorem finalize_monotonic (s s' : ChannelState) (h : finalizeChannel s = some s') :
    s'.status.toNat ≥ s.status.toNat := by
  have h1 := finalize_requires_open s s' h
  have h2 := finalize_produces_finalized s s' h
  simp [h1, h2, Status.toNat]

theorem distribute_monotonic (s s' : ChannelState) (h : distributeChannel s = some s') :
    s'.status.toNat ≥ s.status.toNat := by
  have h1 := distribute_requires_finalized s s' h
  have h2 := distribute_produces_closed s s' h
  simp [h1, h2, Status.toNat]

-- P4: Distribute is one-shot

theorem distribute_once (s : ChannelState) (hfin : s.status = .Finalized) :
    ∃ s', distributeChannel s = some s' ∧
          s'.status = .Closed ∧
          distributeChannel s' = none := by
  exact ⟨{ s with status := .Closed },
    by simp [distributeChannel, hfin],
    rfl,
    by simp [distributeChannel]⟩
