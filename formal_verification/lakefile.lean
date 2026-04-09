import Lake
open Lake DSL

package «FiberProofs» where
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

@[default_target]
lean_lib «Proofs» where
  srcDir := "."
