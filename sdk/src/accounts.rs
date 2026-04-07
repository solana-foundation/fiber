use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use crate::constants::{
    BATCH_FINALIZE_PER_CHANNEL_CU, BATCH_WALKER_BASE_CU, CHANNEL_DATA_SIZE, DISTRIBUTE_BASE_CU,
    DISTRIBUTE_PER_SPLIT_CU, FINALIZE_BASE_CU, ID, IX_BATCH_DISTRIBUTE, IX_BATCH_FINALIZE,
    IX_BATCH_OPEN, IX_DISTRIBUTE, IX_FINALIZE, IX_OPEN, OPEN_BASE_CU, OPERATOR_CHECK_CU,
};

/// A fixed-amount payment split distributed at channel close.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Split {
    pub recipient: Pubkey,
    pub amount: u64,
}

/// Compute the 16-byte distribution hash from recipient + splits.
///
/// Uses SHA-256 truncated to 128 bits. The hash commits to:
///   - the recipient pubkey (32 bytes)
///   - each split's recipient (32 bytes) + amount (8 bytes LE)
///
/// At close time, the caller provides the full distribution config
/// and the program verifies it matches this hash.
pub fn distribution_hash(recipient: &Pubkey, splits: &[Split]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(recipient.as_ref());
    for split in splits {
        hasher.update(split.recipient.as_ref());
        hasher.update(&split.amount.to_le_bytes());
    }
    let result = hasher.finalize();
    let mut truncated = [0u8; 16];
    truncated.copy_from_slice(&result.as_bytes()[..16]);
    truncated
}

/// Instruction to open a new channel.
///
/// The channel account must be pre-created (via create_account_with_seed)
/// with CHANNEL_DATA_SIZE bytes, owned by the fiber program, and zeroed.
/// Token deposit transfer is a separate instruction in the same transaction.
pub struct OpenInstruction {
    pub operator: Pubkey,
    pub channel: Pubkey,
    pub deposit: u64,
    pub distribution_hash: [u8; 16],
}

impl OpenInstruction {
    pub const fn compute_units(&self) -> u32 {
        OPERATOR_CHECK_CU + OPEN_BASE_CU
    }

    pub const fn loaded_accounts_data_size_limit(&self) -> u32 {
        CHANNEL_DATA_SIZE as u32
    }
}

impl From<OpenInstruction> for Instruction {
    fn from(open: OpenInstruction) -> Self {
        // Instruction data: [discriminator: u64, deposit: u64, hash: [u8; 16]]
        let mut data = Vec::with_capacity(32);
        data.push(IX_OPEN);
        data.extend_from_slice(&open.deposit.to_le_bytes());
        data.extend_from_slice(&open.distribution_hash);

        Self {
            program_id: ID,
            accounts: vec![
                AccountMeta::new_readonly(open.operator, true), // operator signer
                AccountMeta::new(open.channel, false),          // channel PDA (writable)
            ],
            data,
        }
    }
}

/// Instruction to finalize a channel (phase 1 of two-phase close).
///
/// Advances the settled watermark and marks the channel as finalized.
/// No token transfers occur — this is the lightweight batchable operation.
pub struct FinalizeInstruction {
    pub operator: Pubkey,
    pub channel: Pubkey,
    pub new_settled: u64,
}

impl FinalizeInstruction {
    pub const fn compute_units(&self) -> u32 {
        OPERATOR_CHECK_CU + FINALIZE_BASE_CU
    }

    pub const fn loaded_accounts_data_size_limit(&self) -> u32 {
        CHANNEL_DATA_SIZE as u32
    }
}

impl From<FinalizeInstruction> for Instruction {
    fn from(finalize: FinalizeInstruction) -> Self {
        // Instruction data: [discriminator: u64, new_settled: u64]
        let mut data = Vec::with_capacity(16);
        data.push(IX_FINALIZE);
        data.extend_from_slice(&finalize.new_settled.to_le_bytes());

        Self {
            program_id: ID,
            accounts: vec![
                AccountMeta::new_readonly(finalize.operator, true),
                AccountMeta::new(finalize.channel, false),
            ],
            data,
        }
    }
}

/// Instruction to distribute a finalized channel's lamports.
///
/// Permissionless — anyone can crank this. The distribution config
/// is verified against the stored 16-byte hash via on-chain SHA-256.
///
/// Accounts: [channel (writable), recipient (writable), payer (writable),
///            split_0 (writable), split_1 (writable), ...]
pub struct DistributeInstruction {
    pub channel: Pubkey,
    pub recipient: Pubkey,
    pub payer: Pubkey,
    pub splits: Vec<Split>,
}

impl DistributeInstruction {
    pub fn compute_units(&self) -> u32 {
        DISTRIBUTE_BASE_CU + DISTRIBUTE_PER_SPLIT_CU * self.splits.len() as u32
    }

    pub fn loaded_accounts_data_size_limit(&self) -> u32 {
        CHANNEL_DATA_SIZE as u32
    }
}

impl From<DistributeInstruction> for Instruction {
    fn from(dist: DistributeInstruction) -> Self {
        // Instruction data: [discriminator: u64, split_amounts: [u64; N]]
        let mut data = Vec::with_capacity(8 + dist.splits.len() * 8);
        data.push(IX_DISTRIBUTE);
        for split in &dist.splits {
            data.extend_from_slice(&split.amount.to_le_bytes());
        }

        let mut accounts = Vec::with_capacity(3 + dist.splits.len());
        accounts.push(AccountMeta::new(dist.channel, false));
        accounts.push(AccountMeta::new(dist.recipient, false));
        accounts.push(AccountMeta::new(dist.payer, false));
        for split in &dist.splits {
            accounts.push(AccountMeta::new(split.recipient, false));
        }

        Self {
            program_id: ID,
            accounts,
            data,
        }
    }
}

/// A channel entry for batch distribute.
pub struct DistributeEntry {
    pub channel: Pubkey,
    pub recipient: Pubkey,
    pub payer: Pubkey,
    pub splits: Vec<Split>,
}

/// Single instruction that distributes N finalized channels. Permissionless.
///
/// Accounts: [ch0, recipient0, payer0, splits0..., ch1, recipient1, payer1, splits1..., ...]
/// Data: [disc: u64 = 5, num_channels: u64,
///        num_splits_0: u64, amounts_0..., num_splits_1: u64, amounts_1..., ...]
pub struct BatchDistributeInstruction {
    pub entries: Vec<DistributeEntry>,
}

impl BatchDistributeInstruction {
    pub fn compute_units(&self) -> u32 {
        let mut cu = BATCH_WALKER_BASE_CU;
        for entry in &self.entries {
            cu += DISTRIBUTE_BASE_CU + DISTRIBUTE_PER_SPLIT_CU * entry.splits.len() as u32;
        }
        cu
    }

    pub fn total_accounts(&self) -> usize {
        self.entries.iter().map(|e| 3 + e.splits.len()).sum()
    }
}

impl From<BatchDistributeInstruction> for Instruction {
    fn from(batch: BatchDistributeInstruction) -> Self {
        let n = batch.entries.len();

        let mut data = Vec::new();
        data.push(IX_BATCH_DISTRIBUTE);
        data.extend_from_slice(&(n as u64).to_le_bytes());
        for entry in &batch.entries {
            data.extend_from_slice(&(entry.splits.len() as u64).to_le_bytes());
            for split in &entry.splits {
                data.extend_from_slice(&split.amount.to_le_bytes());
            }
        }

        let total = batch.entries.iter().map(|e| 3 + e.splits.len()).sum();
        let mut accounts = Vec::with_capacity(total);
        for entry in &batch.entries {
            accounts.push(AccountMeta::new(entry.channel, false));
            accounts.push(AccountMeta::new(entry.recipient, false));
            accounts.push(AccountMeta::new(entry.payer, false));
            for split in &entry.splits {
                accounts.push(AccountMeta::new(split.recipient, false));
            }
        }

        Self {
            program_id: ID,
            accounts,
            data,
        }
    }
}

/// Single instruction that finalizes N channels.
///
/// Accounts: [operator (signer), channel_0, channel_1, ..., channel_N]
/// Data: [disc: u64 = 3, settled_0: u64, settled_1: u64, ...]
pub struct BatchFinalizeInstruction {
    pub operator: Pubkey,
    pub channels: Vec<(Pubkey, u64)>, // (channel address, new_settled)
}

impl BatchFinalizeInstruction {
    pub fn compute_units(&self) -> u32 {
        BATCH_WALKER_BASE_CU + BATCH_FINALIZE_PER_CHANNEL_CU * self.channels.len() as u32
    }

    pub fn loaded_accounts_data_size_limit(&self) -> u32 {
        CHANNEL_DATA_SIZE as u32 * self.channels.len() as u32
    }
}

impl From<BatchFinalizeInstruction> for Instruction {
    fn from(batch: BatchFinalizeInstruction) -> Self {
        let n = batch.channels.len();
        let mut data = Vec::with_capacity(8 + n * 8);
        data.push(IX_BATCH_FINALIZE);
        for (_, settled) in &batch.channels {
            data.extend_from_slice(&settled.to_le_bytes());
        }

        let mut accounts = Vec::with_capacity(1 + n);
        accounts.push(AccountMeta::new_readonly(batch.operator, true));
        for (channel, _) in &batch.channels {
            accounts.push(AccountMeta::new(*channel, false));
        }

        Self {
            program_id: ID,
            accounts,
            data,
        }
    }
}

/// Single instruction that opens N channels.
/// Composable with multi-delegator for operator-driven batch opens.
pub struct BatchOpenInstruction {
    pub operator: Pubkey,
    pub channels: Vec<(Pubkey, u64, [u8; 16])>,
}

impl BatchOpenInstruction {
    pub fn compute_units(&self) -> u32 {
        BATCH_WALKER_BASE_CU + 25 * self.channels.len() as u32
    }

    pub fn loaded_accounts_data_size_limit(&self) -> u32 {
        CHANNEL_DATA_SIZE as u32 * self.channels.len() as u32
    }
}

impl From<BatchOpenInstruction> for Instruction {
    fn from(batch: BatchOpenInstruction) -> Self {
        let n = batch.channels.len();
        let mut data = Vec::with_capacity(1 + n * 24);
        data.push(IX_BATCH_OPEN);
        for (_, deposit, hash) in &batch.channels {
            data.extend_from_slice(&deposit.to_le_bytes());
            data.extend_from_slice(hash);
        }

        let mut accounts = Vec::with_capacity(1 + n);
        accounts.push(AccountMeta::new_readonly(batch.operator, true));
        for (channel, _, _) in &batch.channels {
            accounts.push(AccountMeta::new(*channel, false));
        }

        Self {
            program_id: ID,
            accounts,
            data,
        }
    }
}
