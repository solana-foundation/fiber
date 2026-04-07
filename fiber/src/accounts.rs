// Solana entrypoint input buffer layout constants
const MAX_PERMITTED_DATA_INCREASE: usize = 10240;
const BPF_ALIGN: usize = 8;

// Per-account field offsets (relative to account start)
const ACCT_DUP: usize = 0;
const ACCT_DATA_LEN: usize = 80;
const ACCT_DATA: usize = 88;

const NON_DUP_MARKER: u8 = 0xFF;

use core::mem::MaybeUninit;
use solana_account_view::{AccountView, RuntimeAccount};

/// Maximum accounts we support per instruction.
pub const MAX_ACCOUNTS: usize = 64;

/// Parse accounts from the entrypoint input buffer.
///
/// Returns (accounts array, count, instruction data offset).
///
/// Uses MaybeUninit — only the first `count` slots are initialized.
/// Callers MUST NOT access slots beyond `count`.
///
/// # Safety
/// The caller must ensure `input` points to a valid Solana entrypoint buffer.
pub unsafe fn parse_accounts(input: *mut u8) -> ([AccountView; MAX_ACCOUNTS], usize, usize) {
    let num_accounts = crate::read::<u64>(input, 0) as usize;
    let count = if num_accounts > MAX_ACCOUNTS {
        MAX_ACCOUNTS
    } else {
        num_accounts
    };

    // Only initialize the slots we actually use
    let mut accounts = MaybeUninit::<[AccountView; MAX_ACCOUNTS]>::uninit();
    let accounts_ptr = accounts.as_mut_ptr() as *mut AccountView;

    let mut offset: usize = 8; // skip num_accounts

    for i in 0..count {
        let dup = crate::read::<u8>(input, offset + ACCT_DUP);

        if dup != NON_DUP_MARKER {
            // Duplicate — copy the AccountView from the original slot
            let dup_idx = dup as usize;
            core::ptr::copy_nonoverlapping(accounts_ptr.add(dup_idx), accounts_ptr.add(i), 1);
            offset += 8;
            continue;
        }

        // Zero-copy: AccountView wraps the raw RuntimeAccount pointer
        core::ptr::write(
            accounts_ptr.add(i),
            AccountView::new_unchecked(input.add(offset) as *mut RuntimeAccount),
        );

        // Advance past this account: header + data + realloc padding + align + rent_epoch
        let data_len = crate::read::<u64>(input, offset + ACCT_DATA_LEN) as usize;
        offset = offset + ACCT_DATA + data_len + MAX_PERMITTED_DATA_INCREASE;
        offset = (offset + BPF_ALIGN - 1) & !(BPF_ALIGN - 1);
        offset += 8; // rent_epoch
    }

    // offset now points to instruction_data_len
    let ix_data_offset = offset + 8;

    (accounts.assume_init(), count, ix_data_offset)
}
