use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_hash::Hash;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer as _;
use solana_transaction::Transaction;

use crate::accounts::FinalizeInstruction;
use crate::constants::{
    COMPUTE_BUDGET_DATA_LIMIT_SIZE, COMPUTE_BUDGET_IX_CU, COMPUTE_BUDGET_PROGRAM_SIZE,
    COMPUTE_BUDGET_UNIT_LIMIT_SIZE, COMPUTE_BUDGET_UNIT_PRICE_SIZE, FIBER_PROGRAM_SIZE,
};

/// Transaction builder for batching multiple channel finalizations.
///
/// Modeled after Doppler's Builder — accumulates finalize instructions,
/// auto-calculates CU budget and loaded account data size limits.
pub struct Builder<'a> {
    finalize_ixs: Vec<Instruction>,
    operator: &'a Keypair,
    unit_price: Option<u64>,
    compute_units: u32,
    loaded_account_data_size: u32,
}

impl<'a> Builder<'a> {
    #[must_use]
    pub const fn new(operator: &'a Keypair) -> Self {
        Self {
            operator,
            finalize_ixs: vec![],
            unit_price: None,
            compute_units: COMPUTE_BUDGET_IX_CU * 2, // default 2 compute budget ixs
            loaded_account_data_size: FIBER_PROGRAM_SIZE
                + COMPUTE_BUDGET_PROGRAM_SIZE
                + COMPUTE_BUDGET_UNIT_LIMIT_SIZE
                + COMPUTE_BUDGET_DATA_LIMIT_SIZE
                + 2,
        }
    }

    /// Add a channel finalization to the batch.
    pub fn add_finalize(mut self, channel: solana_pubkey::Pubkey, new_settled: u64) -> Self {
        let finalize_ix = FinalizeInstruction {
            operator: self.operator.pubkey(),
            channel,
            new_settled,
        };

        self.compute_units += finalize_ix.compute_units();
        self.loaded_account_data_size += finalize_ix.loaded_accounts_data_size_limit() * 2;

        self.finalize_ixs.push(finalize_ix.into());

        self
    }

    #[must_use]
    pub const fn with_unit_price(mut self, micro_lamports: u64) -> Self {
        self.unit_price = Some(micro_lamports);
        self
    }

    /// Build the batch finalization transaction.
    #[must_use]
    pub fn build(self, recent_blockhash: Hash) -> Transaction {
        let mut ixs = Vec::with_capacity(self.finalize_ixs.len() + 3);
        let mut loaded_account_data_size = self.loaded_account_data_size;
        let mut compute_units = self.compute_units;

        if let Some(unit_price) = self.unit_price {
            ixs.push(ComputeBudgetInstruction::set_compute_unit_price(unit_price));
            loaded_account_data_size += COMPUTE_BUDGET_UNIT_PRICE_SIZE;
            compute_units += COMPUTE_BUDGET_IX_CU;
        }

        ixs.push(
            ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(loaded_account_data_size),
        );
        ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(
            compute_units,
        ));

        for finalize_ix in self.finalize_ixs {
            ixs.push(finalize_ix);
        }

        Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.operator.pubkey()),
            &[&self.operator],
            recent_blockhash,
        )
    }
}
