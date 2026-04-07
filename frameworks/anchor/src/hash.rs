//! Hash verification utilities for distribution config.

use crate::FiberError;
use anchor_lang::prelude::*;

/// Blake3 syscall — cheaper than SHA-256 on Solana.
#[repr(C)]
struct SolBytes {
    addr: *const u8,
    len: u64,
}

extern "C" {
    fn sol_blake3(vals: *const SolBytes, val_len: u64, hash_result: *mut u8) -> u64;
}

/// Verify the distribution hash matches the provided recipient + splits.
///
/// Hashes `[recipient_pubkey || (split_addr, split_amount)...]` with Blake3
/// and compares the first 16 bytes against the stored hash.
pub fn verify_distribution_hash(
    stored_hash: &[u8; 16],
    recipient_key: &Pubkey,
    split_accounts: &[AccountInfo],
    split_amounts: &[u64],
) -> Result<()> {
    let num_splits = split_amounts.len();

    // Build hash input: recipient pubkey + each split's (pubkey, amount)
    let mut buf = vec![0u8; 32 + num_splits * 40];
    buf[..32].copy_from_slice(recipient_key.as_ref());

    for i in 0..num_splits {
        let offset = 32 + i * 40;
        buf[offset..offset + 32].copy_from_slice(split_accounts[i].key.as_ref());
        buf[offset + 32..offset + 40].copy_from_slice(&split_amounts[i].to_le_bytes());
    }

    // Hash via Blake3 syscall
    let mut hash = [0u8; 32];
    let input = SolBytes {
        addr: buf.as_ptr(),
        len: buf.len() as u64,
    };
    unsafe {
        sol_blake3(&input, 1, hash.as_mut_ptr());
    }

    // Compare first 16 bytes
    require!(hash[..16] == *stored_hash, FiberError::HashMismatch);
    Ok(())
}
