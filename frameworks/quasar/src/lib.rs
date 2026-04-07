//! Quasar implementation of the Fiber payment channel.
//!
//! Quasar sits between Pinocchio and Anchor: derive-based account parsing
//! with signer/writable validation, but zero-copy data access (no automatic
//! deserialization). Account data is still read via byte offsets.
//!
//! Used for CU benchmarking against native, Pinocchio, and Anchor variants.

#![no_std]

use quasar_lang::prelude::*;

declare_id!("11111111111111111111111111111111");

// ---------------------------------------------------------------------------
// Channel data layout (42 bytes, same across all implementations)
// ---------------------------------------------------------------------------

const D: usize = 0; // deposit: u64
const S: usize = 8; // settled: u64
const H: usize = 24; // distribution_hash: [u8; 16]
const ST: usize = 40; // status: u8

const OPEN: u8 = 0;
const FINALIZED: u8 = 1;
const CLOSED: u8 = 2;

// ---------------------------------------------------------------------------
// Blake3 syscall
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a u64 from a byte slice at the given offset.
#[inline(always)]
fn r64(data: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
}

/// Get a mutable AccountView from an UncheckedAccount.
///
/// Quasar's UncheckedAccount is repr(transparent) over AccountView,
/// so this is a zero-cost cast. Needed because Quasar doesn't expose
/// set_lamports() or borrow_unchecked_mut() on its account types.
#[inline(always)]
fn as_view(acct: &mut UncheckedAccount) -> &mut AccountView {
    unsafe { &mut *(acct as *mut UncheckedAccount as *mut AccountView) }
}

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

#[program]
mod fiber_quasar {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn open(ctx: Ctx<Open>, deposit: u64, hash: [u8; 16]) -> Result<(), ProgramError> {
        let data = unsafe { as_view(ctx.accounts.channel).borrow_unchecked_mut() };

        require!(r64(data, D) == 0, ProgramError::AccountAlreadyInitialized);
        require!(deposit > 0, ProgramError::InvalidInstructionData);

        data[D..D + 8].copy_from_slice(&deposit.to_le_bytes());
        data[H..H + 16].copy_from_slice(&hash);
        data[ST] = OPEN;
        Ok(())
    }

    #[instruction(discriminator = 1)]
    pub fn finalize(ctx: Ctx<Finalize>, new_settled: u64) -> Result<(), ProgramError> {
        let data = unsafe { as_view(ctx.accounts.channel).borrow_unchecked_mut() };

        require!(data[ST] == OPEN, ProgramError::InvalidAccountData);
        require!(
            new_settled > r64(data, S),
            ProgramError::InvalidInstructionData
        );
        require!(
            new_settled <= r64(data, D),
            ProgramError::InvalidInstructionData
        );

        data[S..S + 8].copy_from_slice(&new_settled.to_le_bytes());
        data[ST] = FINALIZED;
        Ok(())
    }

    #[instruction(discriminator = 2)]
    pub fn distribute(ctx: CtxWithRemaining<Distribute>) -> Result<(), ProgramError> {
        // Collect remaining accounts (split recipients)
        const UNINIT: core::mem::MaybeUninit<AccountView> = core::mem::MaybeUninit::uninit();
        let mut split_buf = [UNINIT; 32];
        let mut num_splits = 0usize;
        for acct in ctx.remaining_accounts().iter() {
            if num_splits < 32 {
                split_buf[num_splits].write(acct.unwrap());
                num_splits += 1;
            }
        }
        let splits = unsafe {
            core::slice::from_raw_parts_mut(split_buf.as_mut_ptr() as *mut AccountView, num_splits)
        };

        // Read channel state
        let (deposit, settled) = {
            let data = unsafe { ctx.accounts.channel.to_account_view().borrow_unchecked() };
            require!(data[ST] == FINALIZED, ProgramError::InvalidAccountData);
            (r64(data, D), r64(data, S))
        };
        require!(deposit >= settled, ProgramError::InsufficientFunds);

        // Parse split amounts from instruction data
        let ix_data = ctx.data;
        let mut split_total: u64 = 0;
        for i in 0..num_splits {
            split_total += r64(ix_data, i * 8);
        }
        require!(settled >= split_total, ProgramError::InsufficientFunds);
        let recipient_amount = settled - split_total;

        // Verify distribution hash — uses shared fiber::hash::verify
        #[cfg(target_os = "solana")]
        unsafe {
            // Reconstruct channel_data for verify (need the full 42 bytes)
            let ch_data = ctx.accounts.channel.to_account_view().borrow_unchecked();
            let mut addrs = [core::ptr::null::<u8>(); 32];
            for i in 0..num_splits {
                addrs[i] = splits[i].address() as *const _ as *const u8;
            }
            fiber::hash::verify(
                ch_data,
                ctx.accounts.recipient.to_account_view().address() as *const _ as *const u8,
                &addrs[..num_splits],
                ix_data.as_ptr(),
                num_splits,
            );
        }

        // Transfer to splits
        for i in 0..num_splits {
            let amt = r64(ix_data, i * 8);
            let ch = as_view(ctx.accounts.channel);
            ch.set_lamports(ch.lamports() - amt);
            splits[i].set_lamports(splits[i].lamports() + amt);
        }

        // Transfer to recipient
        let ch = as_view(ctx.accounts.channel);
        ch.set_lamports(ch.lamports() - recipient_amount);
        let r = as_view(ctx.accounts.recipient);
        r.set_lamports(r.lamports() + recipient_amount);

        // Refund payer
        let leftover = as_view(ctx.accounts.channel).lamports();
        as_view(ctx.accounts.channel).set_lamports(0);
        let p = as_view(ctx.accounts.payer);
        p.set_lamports(p.lamports() + leftover);

        // Close
        unsafe {
            as_view(ctx.accounts.channel).borrow_unchecked_mut()[ST] = CLOSED;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Account structs
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Open<'info> {
    pub operator: &'info Signer,
    #[account(mut)]
    pub channel: &'info mut UncheckedAccount,
}

#[derive(Accounts)]
pub struct Finalize<'info> {
    pub operator: &'info Signer,
    #[account(mut)]
    pub channel: &'info mut UncheckedAccount,
}

#[derive(Accounts)]
pub struct Distribute<'info> {
    #[account(mut)]
    pub channel: &'info mut UncheckedAccount,
    #[account(mut)]
    pub recipient: &'info mut UncheckedAccount,
    #[account(mut)]
    pub payer: &'info mut UncheckedAccount,
}
