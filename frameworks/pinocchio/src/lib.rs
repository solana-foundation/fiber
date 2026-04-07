//! Pinocchio implementation of the Fiber payment channel.
//!
//! Pinocchio is a zero-copy, zero-alloc Solana program framework. This
//! implementation is used for CU benchmarking against the native, Quasar,
//! and Anchor variants of the same payment channel logic.

use pinocchio::{
    account::AccountView, address::Address, entrypoint, error::ProgramError, ProgramResult,
};

entrypoint!(process_instruction);

// --- Channel account data layout (byte offsets) ---

const DATA_DEPOSIT: usize = 0;
const DATA_SETTLED: usize = 8;
const DATA_DISTRIBUTION_HASH: usize = 24;
const DATA_STATUS: usize = 40;

const STATUS_OPEN: u8 = 0;
const STATUS_FINALIZED: u8 = 1;
const STATUS_CLOSED: u8 = 2;

// --- Instruction dispatch ---

fn process_instruction(
    _program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let (disc, payload) = data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    match disc {
        0 => process_open(accounts, payload),
        1 => process_finalize(accounts, payload),
        2 => process_distribute(accounts, payload),
        3 => process_batch_finalize(accounts, payload),
        4 => process_batch_open(accounts, payload),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

// --- Open: initialize a new payment channel with deposit and distribution hash ---

fn process_open(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if data.len() < 24 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[0].is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let d = unsafe { accounts[1].borrow_unchecked_mut() };
    let cur = u64::from_le_bytes(d[DATA_DEPOSIT..DATA_DEPOSIT + 8].try_into().unwrap());
    if cur != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let dep = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if dep == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    d[DATA_DEPOSIT..DATA_DEPOSIT + 8].copy_from_slice(&dep.to_le_bytes());
    d[DATA_DISTRIBUTION_HASH..DATA_DISTRIBUTION_HASH + 16].copy_from_slice(&data[8..24]);
    d[DATA_STATUS] = STATUS_OPEN;
    Ok(())
}

// --- Finalize: lock the channel with a new settled amount ---

fn process_finalize(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if data.len() < 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[0].is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let d = unsafe { accounts[1].borrow_unchecked_mut() };
    if d[DATA_STATUS] != STATUS_OPEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let cur = u64::from_le_bytes(d[DATA_SETTLED..DATA_SETTLED + 8].try_into().unwrap());
    let new = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if new <= cur {
        return Err(ProgramError::InvalidInstructionData);
    }
    let dep = u64::from_le_bytes(d[DATA_DEPOSIT..DATA_DEPOSIT + 8].try_into().unwrap());
    if new > dep {
        return Err(ProgramError::InvalidInstructionData);
    }

    d[DATA_SETTLED..DATA_SETTLED + 8].copy_from_slice(&new.to_le_bytes());
    d[DATA_STATUS] = STATUS_FINALIZED;
    Ok(())
}

// --- Distribute: verify hash, pay splits + recipient, refund payer, close channel ---

fn process_distribute(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if accounts.len() < 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let num_splits = accounts.len() - 3;

    let (deposit, settled) = {
        let d = unsafe { accounts[0].borrow_unchecked() };
        if d[DATA_STATUS] != STATUS_FINALIZED {
            return Err(ProgramError::InvalidAccountData);
        }
        (
            u64::from_le_bytes(d[DATA_DEPOSIT..DATA_DEPOSIT + 8].try_into().unwrap()),
            u64::from_le_bytes(d[DATA_SETTLED..DATA_SETTLED + 8].try_into().unwrap()),
        )
    };

    let mut split_total: u64 = 0;
    for i in 0..num_splits {
        split_total += u64::from_le_bytes(data[i * 8..i * 8 + 8].try_into().unwrap());
    }
    if settled < split_total {
        return Err(ProgramError::InsufficientFunds);
    }
    let recipient_amount = settled - split_total;
    if deposit < settled {
        return Err(ProgramError::InsufficientFunds);
    }

    // Hash verification — uses shared fiber::hash::verify
    #[cfg(target_os = "solana")]
    unsafe {
        let channel_data = accounts[0].borrow_unchecked();
        let mut split_addrs = [core::ptr::null::<u8>(); 32];
        for i in 0..num_splits {
            split_addrs[i] = accounts[3 + i].address().as_ref().as_ptr();
        }
        fiber::hash::verify(
            channel_data,
            accounts[1].address().as_ref().as_ptr(),
            &split_addrs[..num_splits],
            data.as_ptr(),
            num_splits,
        );
    }

    for i in 0..num_splits {
        let amt = u64::from_le_bytes(data[i * 8..i * 8 + 8].try_into().unwrap());
        accounts[0].set_lamports(accounts[0].lamports() - amt);
        accounts[3 + i].set_lamports(accounts[3 + i].lamports() + amt);
    }
    accounts[0].set_lamports(accounts[0].lamports() - recipient_amount);
    accounts[1].set_lamports(accounts[1].lamports() + recipient_amount);
    let rem = accounts[0].lamports();
    accounts[0].set_lamports(0);
    accounts[2].set_lamports(accounts[2].lamports() + rem);

    unsafe {
        accounts[0].borrow_unchecked_mut()[DATA_STATUS] = STATUS_CLOSED;
    }
    Ok(())
}

// --- Batch finalize: finalize multiple channels in one transaction ---
/// Data layout: [settled_0: u64, settled_1: u64, ...]
/// Accounts: [operator (signer), channel_0, channel_1, ...]
fn process_batch_finalize(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[0].is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let num_channels = accounts.len() - 1;

    for i in 0..num_channels {
        let new_settled = u64::from_le_bytes(data[i * 8..i * 8 + 8].try_into().unwrap());

        let d = unsafe { accounts[1 + i].borrow_unchecked_mut() };
        if d[DATA_STATUS] != STATUS_OPEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let cur = u64::from_le_bytes(d[DATA_SETTLED..DATA_SETTLED + 8].try_into().unwrap());
        if new_settled <= cur {
            return Err(ProgramError::InvalidInstructionData);
        }
        let dep = u64::from_le_bytes(d[DATA_DEPOSIT..DATA_DEPOSIT + 8].try_into().unwrap());
        if new_settled > dep {
            return Err(ProgramError::InvalidInstructionData);
        }

        d[DATA_SETTLED..DATA_SETTLED + 8].copy_from_slice(&new_settled.to_le_bytes());
        d[DATA_STATUS] = STATUS_FINALIZED;
    }
    Ok(())
}

// --- Batch open: initialize multiple channels in one transaction ---
/// Data layout: [(deposit_0: u64, hash_0: [u8;16]), ...]
/// Accounts: [operator (signer), channel_0, channel_1, ...]
fn process_batch_open(accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[0].is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let num_channels = accounts.len() - 1;

    for i in 0..num_channels {
        let offset = i * 24;
        let dep = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        if dep == 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let d = unsafe { accounts[1 + i].borrow_unchecked_mut() };
        let cur = u64::from_le_bytes(d[DATA_DEPOSIT..DATA_DEPOSIT + 8].try_into().unwrap());
        if cur != 0 {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        d[DATA_DEPOSIT..DATA_DEPOSIT + 8].copy_from_slice(&dep.to_le_bytes());
        d[DATA_DISTRIBUTION_HASH..DATA_DISTRIBUTION_HASH + 16]
            .copy_from_slice(&data[offset + 8..offset + 24]);
        d[DATA_STATUS] = STATUS_OPEN;
    }
    Ok(())
}
