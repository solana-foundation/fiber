//! Blake3 hash verification for distribution configs.
//!
//! Hashes `[recipient_pubkey || (split_addr, split_amount)...]` and compares
//! the first 16 bytes against the stored truncated hash.

#[repr(C)]
pub struct SolBytes {
    pub addr: *const u8,
    pub len: u64,
}

#[cfg(target_os = "solana")]
extern "C" {
    pub fn sol_blake3(vals: *const SolBytes, val_len: u64, hash_result: *mut u8) -> u64;
}

/// Distribution hash offset within channel data.
#[allow(dead_code)]
const H: usize = 24;

/// Verify the distribution hash using Blake3 syscall.
///
/// Builds the hash input from recipient + split addresses/amounts,
/// hashes with Blake3, and compares first 16 bytes as two u64s.
#[cfg(target_os = "solana")]
pub unsafe fn verify(
    channel_data: &[u8],
    recipient_addr: *const u8,
    split_addrs: &[*const u8],
    split_amounts_ptr: *const u8,
    num_splits: usize,
) {
    use core::mem::MaybeUninit;
    let mut hash_buf = MaybeUninit::<[u8; 32 + 32 * 40]>::uninit();
    let buf_ptr = hash_buf.as_mut_ptr() as *mut u8;
    let mut pos: usize = 0;

    core::ptr::copy_nonoverlapping(recipient_addr, buf_ptr.add(pos), 32);
    pos += 32;

    let mut s = 0;
    while s < num_splits {
        core::ptr::copy_nonoverlapping(split_addrs[s], buf_ptr.add(pos), 32);
        pos += 32;
        core::ptr::copy_nonoverlapping(split_amounts_ptr.add(s * 8), buf_ptr.add(pos), 8);
        pos += 8;
        s += 1;
    }

    let mut full_hash = MaybeUninit::<[u8; 32]>::uninit();
    let input_slice = SolBytes {
        addr: buf_ptr,
        len: pos as u64,
    };
    sol_blake3(
        &input_slice as *const SolBytes,
        1,
        full_hash.as_mut_ptr() as *mut u8,
    );

    let hash_ptr = full_hash.as_ptr() as *const u64;
    let stored_ptr = channel_data.as_ptr().add(H) as *const u64;

    if *hash_ptr != *stored_ptr || *hash_ptr.add(1) != *stored_ptr.add(1) {
        crate::fail(15);
    }
}
