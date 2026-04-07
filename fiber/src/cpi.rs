//! Ultra-lightweight CPI helpers for SPL Token transfers.

use core::mem::MaybeUninit;
use solana_account_view::AccountView;
use solana_instruction_view::{
    cpi::{invoke_signed, Signer},
    InstructionAccount, InstructionView,
};
use solana_program_error::ProgramResult;

/// SPL Token Transfer instruction discriminator.
const SPL_TRANSFER: u8 = 3;

/// Execute a signed SPL Token transfer via CPI.
pub fn transfer_signed(
    source: &AccountView,
    destination: &AccountView,
    authority: &AccountView,
    token_program: &AccountView,
    amount: u64,
    signer_seeds: &[Signer],
) -> ProgramResult {
    let accounts = [
        InstructionAccount::writable(source.address()),
        InstructionAccount::writable(destination.address()),
        InstructionAccount::readonly_signer(authority.address()),
    ];

    let account_infos = [source, destination, authority, token_program];

    let mut ix_data = MaybeUninit::<[u8; 9]>::uninit();
    unsafe {
        let ptr = ix_data.as_mut_ptr() as *mut u8;
        core::ptr::write(ptr, SPL_TRANSFER);
        core::ptr::copy_nonoverlapping(amount.to_le_bytes().as_ptr(), ptr.add(1), 8);
    }

    let instruction = InstructionView {
        program_id: token_program.address(),
        accounts: &accounts,
        data: unsafe { core::slice::from_raw_parts(ix_data.as_ptr() as *const u8, 9) },
    };

    invoke_signed(&instruction, &account_infos, signer_seeds)
}

/// Authority PDA seed prefix.
pub const AUTHORITY_SEED: &[u8] = b"authority";
