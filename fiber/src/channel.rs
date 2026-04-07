#![allow(clippy::missing_safety_doc)]
#![allow(unused_must_use)]

// Channel account data layout (42 bytes)
//
// Account data layout (relative to data start):
//   [+0]  deposit:            u64  (8)
//   [+8]  settled:            u64  (8)
//   [+16] close_requested_at: i64  (8)
//   [+24] distribution_hash:  [16] (16) — truncated Blake3
//   [+40] status:             u8   (1)  — 0=Open, 1=Finalized, 2=Closed
//   [+41] bump:               u8   (1)

// --- Hardcoded offsets for open/finalize (2-account layout) ---
const CHANNEL_DEPOSIT: usize = 0x28c0;
const CHANNEL_SETTLED: usize = 0x28c8;
#[allow(dead_code)]
const CHANNEL_CLOSE_REQUESTED_AT: usize = 0x28d0;
const CHANNEL_DISTRIBUTION_HASH: usize = 0x28d8;
const CHANNEL_STATUS: usize = 0x28e8;
#[allow(dead_code)]
const CHANNEL_BUMP: usize = 0x28e9;

pub const IX_DISCRIMINATOR: usize = 0x5100;
const IX_OPEN_DEPOSIT: usize = 0x5101;
const IX_OPEN_HASH: usize = 0x5109;
const IX_FINALIZE_SETTLED: usize = 0x5101;

// --- Data-relative offsets ---
const D: usize = 0; // deposit
const S: usize = 8; // settled
const H: usize = 24; // distribution_hash
const ST: usize = 40; // status

const STATUS_OPEN: u8 = 0;
const STATUS_FINALIZED: u8 = 1;
const STATUS_CLOSED: u8 = 2;

pub const IX_OPEN: u8 = 0;
pub const IX_FINALIZE: u8 = 1;
pub const IX_DISTRIBUTE: u8 = 2;
pub const IX_BATCH_FINALIZE: u8 = 3;
pub const IX_BATCH_OPEN: u8 = 4;
pub const IX_BATCH_DISTRIBUTE: u8 = 5;
pub const IX_DISTRIBUTE_TOKEN: u8 = 6;

use solana_account_view::AccountView;
use solana_instruction_view::cpi::Signer;

/// Read u64 from a data slice at offset. No bounds check.
#[inline(always)]
unsafe fn r64(data: &[u8], off: usize) -> u64 {
    *(data.as_ptr().add(off) as *const u64)
}

pub struct Channel;

impl Channel {
    // --- Open (hardcoded 2-account offsets) ---

    #[inline(always)]
    pub unsafe fn open(ptr: *mut u8) {
        let current_deposit = crate::read::<u64>(ptr, CHANNEL_DEPOSIT);
        if current_deposit != 0 {
            crate::fail(2);
        }

        let deposit = crate::read::<u64>(ptr, IX_OPEN_DEPOSIT);
        if deposit == 0 {
            crate::fail(3);
        }

        crate::write(ptr, CHANNEL_DEPOSIT, deposit);
        core::ptr::copy_nonoverlapping(
            ptr.add(IX_OPEN_HASH),
            ptr.add(CHANNEL_DISTRIBUTION_HASH),
            16,
        );
        crate::write::<u8>(ptr, CHANNEL_STATUS, STATUS_OPEN);
    }

    // --- Finalize (hardcoded 2-account offsets) ---

    #[inline(always)]
    pub unsafe fn finalize(ptr: *mut u8) {
        let status = crate::read::<u8>(ptr, CHANNEL_STATUS);
        if status != STATUS_OPEN {
            crate::fail(4);
        }

        let current_settled = crate::read::<u64>(ptr, CHANNEL_SETTLED);
        let new_settled = crate::read::<u64>(ptr, IX_FINALIZE_SETTLED);
        if new_settled <= current_settled {
            crate::fail(5);
        }

        let deposit = crate::read::<u64>(ptr, CHANNEL_DEPOSIT);
        if new_settled > deposit {
            crate::fail(6);
        }

        crate::write(ptr, CHANNEL_SETTLED, new_settled);
        crate::write(ptr, CHANNEL_STATUS, STATUS_FINALIZED);
    }

    // --- Distribute (lamport, single channel) ---

    pub unsafe fn distribute(
        ptr: *mut u8,
        accounts: &mut [AccountView],
        num_accounts: usize,
        ix_offset: usize,
    ) {
        if num_accounts < 3 {
            crate::fail(10);
        }
        let num_splits = num_accounts - 3;

        // Single borrow — read all channel fields from one slice
        let ch_data = accounts[0].borrow_unchecked();
        let deposit = r64(ch_data, D);
        let settled = r64(ch_data, S);
        if ch_data[ST] != STATUS_FINALIZED {
            crate::fail(11);
        }

        let split_amounts_ptr = ptr.add(ix_offset + 1);

        let mut split_total: u64 = 0;
        {
            let mut i = 0;
            while i < num_splits {
                split_total += crate::read::<u64>(split_amounts_ptr, i * 8);
                i += 1;
            }
        }

        if settled < split_total {
            crate::fail(12);
        }
        let recipient_amount = settled - split_total;
        if recipient_amount == 0 && settled > 0 {
            crate::fail(13);
        }
        if deposit < settled {
            crate::fail(14);
        }

        #[cfg(target_os = "solana")]
        {
            let mut split_addrs = [core::ptr::null::<u8>(); 32];
            let mut i = 0;
            while i < num_splits {
                split_addrs[i] = accounts[3 + i].address() as *const _ as *const u8;
                i += 1;
            }
            crate::hash::verify(
                ch_data,
                accounts[1].address() as *const _ as *const u8,
                &split_addrs[..num_splits],
                split_amounts_ptr,
                num_splits,
            );
        }

        {
            let mut i = 0;
            while i < num_splits {
                let amt = crate::read::<u64>(split_amounts_ptr, i * 8);
                accounts[0].set_lamports(accounts[0].lamports() - amt);
                accounts[3 + i].set_lamports(accounts[3 + i].lamports() + amt);
                i += 1;
            }
        }

        accounts[0].set_lamports(accounts[0].lamports() - recipient_amount);
        accounts[1].set_lamports(accounts[1].lamports() + recipient_amount);

        let remaining = accounts[0].lamports();
        accounts[0].set_lamports(0);
        accounts[2].set_lamports(accounts[2].lamports() + remaining);

        accounts[0].borrow_unchecked_mut()[ST] = STATUS_CLOSED;
    }

    // --- Distribute Token (CPI) ---

    pub unsafe fn distribute_token(
        ptr: *mut u8,
        accounts: &mut [AccountView],
        num_accounts: usize,
        ix_offset: usize,
    ) {
        if num_accounts < 6 {
            crate::fail(50);
        }
        let num_splits = num_accounts - 6;

        let ch_data = accounts[0].borrow_unchecked();
        let deposit = r64(ch_data, D);
        let settled = r64(ch_data, S);
        if ch_data[ST] != STATUS_FINALIZED {
            crate::fail(51);
        }

        let bump = crate::read::<u8>(ptr.add(ix_offset), 1);
        let split_amounts_ptr = ptr.add(ix_offset + 2);

        let mut split_total: u64 = 0;
        {
            let mut i = 0;
            while i < num_splits {
                split_total += crate::read::<u64>(split_amounts_ptr, i * 8);
                i += 1;
            }
        }

        if settled < split_total {
            crate::fail(52);
        }
        let recipient_amount = settled - split_total;
        if recipient_amount == 0 && settled > 0 {
            crate::fail(53);
        }
        if deposit < settled {
            crate::fail(54);
        }

        #[cfg(target_os = "solana")]
        {
            let mut split_addrs = [core::ptr::null::<u8>(); 32];
            let mut i = 0;
            while i < num_splits {
                split_addrs[i] = accounts[6 + i].address() as *const _ as *const u8;
                i += 1;
            }
            crate::hash::verify(
                ch_data,
                accounts[4].address() as *const _ as *const u8,
                &split_addrs[..num_splits],
                split_amounts_ptr,
                num_splits,
            );
        }

        let bump_bytes = [bump];
        use solana_instruction_view::cpi::Seed;
        let seeds = [
            Seed::from(crate::cpi::AUTHORITY_SEED),
            Seed::from(&bump_bytes),
        ];
        let signer = Signer::from(&seeds);
        let signers = [signer];

        {
            let mut i = 0;
            while i < num_splits {
                let amt = crate::read::<u64>(split_amounts_ptr, i * 8);
                crate::cpi::transfer_signed(
                    &accounts[1],
                    &accounts[6 + i],
                    &accounts[2],
                    &accounts[3],
                    amt,
                    &signers,
                );
                i += 1;
            }
        }

        if recipient_amount > 0 {
            crate::cpi::transfer_signed(
                &accounts[1],
                &accounts[4],
                &accounts[2],
                &accounts[3],
                recipient_amount,
                &signers,
            );
        }

        let payer_refund = deposit - settled;
        if payer_refund > 0 {
            crate::cpi::transfer_signed(
                &accounts[1],
                &accounts[5],
                &accounts[2],
                &accounts[3],
                payer_refund,
                &signers,
            );
        }

        accounts[0].borrow_unchecked_mut()[ST] = STATUS_CLOSED;
    }

    // --- Batch Distribute ---

    pub unsafe fn batch_distribute(
        ptr: *mut u8,
        accounts: &mut [AccountView],
        _num_accounts: usize,
        ix_offset: usize,
    ) {
        let ix_ptr = ptr.add(ix_offset);
        let num_channels = crate::read::<u64>(ix_ptr, 1) as usize;

        let mut acct_idx: usize = 0;
        let mut data_off: usize = 9;

        let mut ch_i = 0;
        while ch_i < num_channels {
            let num_splits = crate::read::<u64>(ix_ptr, data_off) as usize;
            data_off += 8;
            let split_amounts_off = data_off;

            // Single borrow per channel
            let ch_data = accounts[acct_idx].borrow_unchecked();
            if ch_data[ST] != STATUS_FINALIZED {
                crate::fail(40);
            }
            let deposit = r64(ch_data, D);
            let settled = r64(ch_data, S);

            let mut split_total: u64 = 0;
            {
                let mut s = 0;
                while s < num_splits {
                    split_total += crate::read::<u64>(ix_ptr, split_amounts_off + s * 8);
                    s += 1;
                }
            }

            if settled < split_total {
                crate::fail(41);
            }
            let recipient_amount = settled - split_total;
            if deposit < settled {
                crate::fail(42);
            }

            #[cfg(target_os = "solana")]
            {
                let mut split_addrs = [core::ptr::null::<u8>(); 32];
                let mut s = 0;
                while s < num_splits {
                    split_addrs[s] = accounts[acct_idx + 3 + s].address() as *const _ as *const u8;
                    s += 1;
                }
                crate::hash::verify(
                    ch_data,
                    accounts[acct_idx + 1].address() as *const _ as *const u8,
                    &split_addrs[..num_splits],
                    ix_ptr.add(split_amounts_off),
                    num_splits,
                );
            }

            {
                let mut s = 0;
                while s < num_splits {
                    let amt = crate::read::<u64>(ix_ptr, split_amounts_off + s * 8);
                    accounts[acct_idx].set_lamports(accounts[acct_idx].lamports() - amt);
                    accounts[acct_idx + 3 + s]
                        .set_lamports(accounts[acct_idx + 3 + s].lamports() + amt);
                    s += 1;
                }
            }

            accounts[acct_idx].set_lamports(accounts[acct_idx].lamports() - recipient_amount);
            accounts[acct_idx + 1]
                .set_lamports(accounts[acct_idx + 1].lamports() + recipient_amount);

            let remaining = accounts[acct_idx].lamports();
            accounts[acct_idx].set_lamports(0);
            accounts[acct_idx + 2].set_lamports(accounts[acct_idx + 2].lamports() + remaining);

            accounts[acct_idx].borrow_unchecked_mut()[ST] = STATUS_CLOSED;

            acct_idx += 3 + num_splits;
            data_off += num_splits * 8;
            ch_i += 1;
        }
    }

    // --- Batch Finalize ---

    pub unsafe fn batch_finalize(
        ptr: *mut u8,
        accounts: &mut [AccountView],
        num_accounts: usize,
        ix_offset: usize,
    ) {
        if num_accounts < 2 {
            crate::fail(20);
        }
        let num_channels = num_accounts - 1;
        let settled_ptr = ptr.add(ix_offset + 1);

        let mut i = 0;
        while i < num_channels {
            let new_settled = crate::read::<u64>(settled_ptr, i * 8);

            // Single borrow to read all fields
            let ch_data = accounts[1 + i].borrow_unchecked();
            if ch_data[ST] != STATUS_OPEN {
                crate::fail(21);
            }
            let current_settled = r64(ch_data, S);
            if new_settled <= current_settled {
                crate::fail(22);
            }
            let deposit = r64(ch_data, D);
            if new_settled > deposit {
                crate::fail(23);
            }

            let data = accounts[1 + i].borrow_unchecked_mut();
            data[S..S + 8].copy_from_slice(&new_settled.to_le_bytes());
            data[ST] = STATUS_FINALIZED;

            i += 1;
        }
    }

    // --- Batch Open ---

    /// Open N channels in one instruction.
    /// Composable with multi-delegator for operator-driven batch opens.
    /// Accounts: [operator (signer), channel_0, channel_1, ..., channel_N]
    /// Data: [disc, (deposit_0: u64, hash_0: [u8;16]), ...]
    pub unsafe fn batch_open(
        ptr: *mut u8,
        accounts: &mut [AccountView],
        num_accounts: usize,
        ix_offset: usize,
    ) {
        if num_accounts < 2 {
            crate::fail(30);
        }
        let num_channels = num_accounts - 1;
        let entries_ptr = ptr.add(ix_offset + 1);

        let mut i = 0;
        while i < num_channels {
            let entry_offset = i * 24;

            let ch_data = accounts[1 + i].borrow_unchecked();
            if r64(ch_data, D) != 0 {
                crate::fail(31);
            }

            let deposit = crate::read::<u64>(entries_ptr, entry_offset);
            if deposit == 0 {
                crate::fail(32);
            }

            let data = accounts[1 + i].borrow_unchecked_mut();
            data[D..D + 8].copy_from_slice(&deposit.to_le_bytes());
            core::ptr::copy_nonoverlapping(
                entries_ptr.add(entry_offset + 8),
                data.as_mut_ptr().add(H),
                16,
            );
            data[ST] = STATUS_OPEN;

            i += 1;
        }
    }
}
