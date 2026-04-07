use solana_pubkey::Pubkey;

// TODO: Replace with deployed program ID
pub const ID: Pubkey = Pubkey::new_from_array([0u8; 32]);

/// Channel account data size (bytes).
pub const CHANNEL_DATA_SIZE: usize = 42;

/// Distribution hash size (truncated SHA-256).
#[allow(dead_code)]
pub const DISTRIBUTION_HASH_SIZE: usize = 16;

// Instruction discriminators (u8 — 1 byte, no padding)
pub const IX_OPEN: u8 = 0;
pub const IX_FINALIZE: u8 = 1;
pub const IX_DISTRIBUTE: u8 = 2;
pub const IX_BATCH_FINALIZE: u8 = 3;
pub const IX_BATCH_OPEN: u8 = 4;
pub const IX_BATCH_DISTRIBUTE: u8 = 5;
#[allow(dead_code)]
pub const IX_DISTRIBUTE_TOKEN: u8 = 6;

// CU costs
pub(crate) const OPERATOR_CHECK_CU: u32 = 6;
pub(crate) const OPEN_BASE_CU: u32 = 20;
pub(crate) const FINALIZE_BASE_CU: u32 = 20;
pub(crate) const DISTRIBUTE_BASE_CU: u32 = 200; // hash verify + lamport transfers
pub(crate) const DISTRIBUTE_PER_SPLIT_CU: u32 = 20; // per additional split

// Batch CU costs (account walker overhead + per-channel logic)
pub(crate) const BATCH_WALKER_BASE_CU: u32 = 50;
pub(crate) const BATCH_FINALIZE_PER_CHANNEL_CU: u32 = 20;

pub(crate) const COMPUTE_BUDGET_IX_CU: u32 = 150;
pub(crate) const COMPUTE_BUDGET_UNIT_PRICE_SIZE: u32 = 9;
pub(crate) const COMPUTE_BUDGET_UNIT_LIMIT_SIZE: u32 = 5;
pub(crate) const COMPUTE_BUDGET_DATA_LIMIT_SIZE: u32 = 5;
pub(crate) const COMPUTE_BUDGET_PROGRAM_SIZE: u32 = 22;
pub(crate) const FIBER_PROGRAM_SIZE: u32 = 36;
