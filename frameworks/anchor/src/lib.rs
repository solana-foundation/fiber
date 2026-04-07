#![allow(unexpected_cfgs, clippy::needless_lifetimes)]

//! Anchor implementation of the Fiber payment channel.
//!
//! Uses Anchor's typed account system and automatic (de)serialization.
//! This is the idiomatic Rust version — no raw byte offsets, no unsafe,
//! no manual pointer arithmetic. Used for CU benchmarking against the
//! native, Pinocchio, and Quasar variants.

mod hash;

use anchor_lang::prelude::*;
use hash::verify_distribution_hash;

declare_id!("11111111111111111111111111111111");

/// On-chain state for a payment channel (42 bytes).
#[account]
#[derive(InitSpace)]
pub struct Channel {
    pub deposit: u64,
    pub settled: u64,
    pub close_requested_at: i64,
    pub distribution_hash: [u8; 16],
    pub status: u8,
    pub bump: u8,
}

impl Channel {
    pub const STATUS_OPEN: u8 = 0;
    pub const STATUS_FINALIZED: u8 = 1;
    pub const STATUS_CLOSED: u8 = 2;
}

// ---------------------------------------------------------------------------
// Instructions
// ---------------------------------------------------------------------------

#[program]
pub mod fiber_anchor {
    use super::*;

    /// Initialize a new payment channel with a deposit amount and
    /// a 16-byte Blake3 hash of the distribution config (recipient + splits).
    pub fn open(ctx: Context<Open>, deposit: u64, hash: [u8; 16]) -> Result<()> {
        require!(deposit > 0, FiberError::ZeroDeposit);

        let channel = &mut ctx.accounts.channel;
        require!(channel.deposit == 0, FiberError::AlreadyInitialized);

        channel.deposit = deposit;
        channel.distribution_hash = hash;
        channel.status = Channel::STATUS_OPEN;
        Ok(())
    }

    /// Advance the settled watermark and mark the channel as finalized.
    /// After finalization, no more vouchers are accepted.
    pub fn finalize(ctx: Context<Finalize>, new_settled: u64) -> Result<()> {
        let channel = &mut ctx.accounts.channel;

        require!(channel.status == Channel::STATUS_OPEN, FiberError::NotOpen);
        require!(
            new_settled > channel.settled,
            FiberError::SettledNotIncreasing
        );
        require!(new_settled <= channel.deposit, FiberError::ExceedsDeposit);

        channel.settled = new_settled;
        channel.status = Channel::STATUS_FINALIZED;
        Ok(())
    }

    /// Distribute a finalized channel's lamports to recipient, split
    /// recipients (via remaining_accounts), and refund the payer.
    /// Verifies the distribution hash on-chain via Blake3 syscall.
    pub fn distribute(ctx: Context<Distribute>, split_amounts: Vec<u64>) -> Result<()> {
        let remaining = &ctx.remaining_accounts;
        require!(
            remaining.len() == split_amounts.len(),
            FiberError::SplitCountMismatch
        );

        let channel = &mut ctx.accounts.channel;
        require!(
            channel.status == Channel::STATUS_FINALIZED,
            FiberError::NotFinalized
        );
        require!(
            channel.deposit >= channel.settled,
            FiberError::ExceedsDeposit
        );

        let split_total: u64 = split_amounts.iter().sum();
        require!(channel.settled >= split_total, FiberError::ExceedsDeposit);
        let recipient_amount = channel.settled - split_total;

        // Verify distribution hash
        verify_distribution_hash(
            &channel.distribution_hash,
            ctx.accounts.recipient.key,
            remaining,
            &split_amounts,
        )?;

        // Transfer to each split recipient
        let channel_info = channel.to_account_info();
        for (i, amt) in split_amounts.iter().enumerate() {
            **channel_info.lamports.borrow_mut() -= amt;
            **remaining[i].lamports.borrow_mut() += amt;
        }

        // Transfer to recipient
        **channel_info.lamports.borrow_mut() -= recipient_amount;
        **ctx.accounts.recipient.lamports.borrow_mut() += recipient_amount;

        // Refund payer (remaining lamports including rent)
        let leftover = **channel_info.lamports.borrow();
        **channel_info.lamports.borrow_mut() = 0;
        **ctx.accounts.payer.lamports.borrow_mut() += leftover;

        channel.status = Channel::STATUS_CLOSED;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Account validation structs
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Open<'info> {
    pub operator: Signer<'info>,

    #[account(mut)]
    pub channel: Account<'info, Channel>,
}

#[derive(Accounts)]
pub struct Finalize<'info> {
    pub operator: Signer<'info>,

    #[account(mut)]
    pub channel: Account<'info, Channel>,
}

#[derive(Accounts)]
pub struct Distribute<'info> {
    #[account(mut)]
    pub channel: Account<'info, Channel>,

    /// CHECK: receives settled amount minus splits
    #[account(mut)]
    pub recipient: AccountInfo<'info>,

    /// CHECK: receives deposit - settled refund
    #[account(mut)]
    pub payer: AccountInfo<'info>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[error_code]
pub enum FiberError {
    #[msg("Channel already initialized")]
    AlreadyInitialized,

    #[msg("Deposit must be greater than zero")]
    ZeroDeposit,

    #[msg("Channel is not open")]
    NotOpen,

    #[msg("Settled amount must increase")]
    SettledNotIncreasing,

    #[msg("Amount exceeds deposit")]
    ExceedsDeposit,

    #[msg("Channel is not finalized")]
    NotFinalized,

    #[msg("Distribution hash mismatch")]
    HashMismatch,

    #[msg("Split count does not match remaining accounts")]
    SplitCountMismatch,
}
