//! Shared test helpers for all framework implementations.
//!
//! Each test function takes a `program_id` and `binary_path` so the same
//! test logic can run against native, pinocchio, and quasar binaries.

use crate::{
    distribution_hash, BatchDistributeInstruction, BatchFinalizeInstruction, BatchOpenInstruction,
    DistributeEntry, DistributeInstruction, FinalizeInstruction, OpenInstruction, Split,
    CHANNEL_DATA_SIZE,
};
use mollusk_svm::result::Check;
use mollusk_svm::Mollusk;
use solana_account::{Account, ReadableAccount};
use solana_clock::Epoch;
use solana_instruction::Instruction;
use solana_pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Account builders
// ---------------------------------------------------------------------------

pub fn signer_account(key: Pubkey) -> (Pubkey, Account) {
    (
        key,
        Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
    )
}

pub fn empty_channel(
    mollusk: &mut Mollusk,
    program_id: &Pubkey,
    operator: Pubkey,
    seed: &str,
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&operator, seed, program_id).unwrap();
    let lamports = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    (
        key,
        Account {
            lamports,
            data: vec![0u8; CHANNEL_DATA_SIZE],
            owner: *program_id,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

pub fn finalized_channel(
    mollusk: &mut Mollusk,
    program_id: &Pubkey,
    operator: Pubkey,
    seed: &str,
    deposit: u64,
    settled: u64,
    hash: [u8; 16],
    extra_lamports: u64,
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&operator, seed, program_id).unwrap();
    let rent = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    let mut data = vec![0u8; CHANNEL_DATA_SIZE];
    data[0..8].copy_from_slice(&deposit.to_le_bytes());
    data[8..16].copy_from_slice(&settled.to_le_bytes());
    data[24..40].copy_from_slice(&hash);
    data[40] = 1; // Finalized
    (
        key,
        Account {
            lamports: rent + extra_lamports,
            data,
            owner: *program_id,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

pub fn open_channel(
    mollusk: &mut Mollusk,
    program_id: &Pubkey,
    operator: Pubkey,
    seed: &str,
    deposit: u64,
    hash: [u8; 16],
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&operator, seed, program_id).unwrap();
    let lamports = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    let mut data = vec![0u8; CHANNEL_DATA_SIZE];
    data[0..8].copy_from_slice(&deposit.to_le_bytes());
    data[24..40].copy_from_slice(&hash);
    (
        key,
        Account {
            lamports,
            data,
            owner: *program_id,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

pub fn read_channel_state(data: &[u8]) -> (u64, u64, i64, [u8; 16], u8) {
    let deposit = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let settled = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let close_req = i64::from_le_bytes(data[16..24].try_into().unwrap());
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&data[24..40]);
    let status = data[40];
    (deposit, settled, close_req, hash, status)
}

// ---------------------------------------------------------------------------
// Test functions
// ---------------------------------------------------------------------------

pub fn test_open(program_id: &Pubkey, binary_path: &str) {
    let mut mollusk = Mollusk::new(program_id, binary_path);

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = signer_account(operator);
    let (channel_key, channel_account) =
        empty_channel(&mut mollusk, program_id, operator, "channel-0");

    let recipient = Pubkey::new_unique();
    let splits = [Split {
        recipient: Pubkey::new_unique(),
        amount: 50_000,
    }];
    let hash = distribution_hash(&recipient, &splits);

    let open_ix: Instruction = OpenInstruction {
        operator: operator_key,
        channel: channel_key,
        deposit: 1_000_000,
        distribution_hash: hash,
    }
    .into();

    let result = mollusk.process_and_validate_instruction(
        &open_ix,
        &[
            (operator_key, operator_account),
            (channel_key, channel_account),
        ],
        &[Check::success()],
    );

    let updated = result.get_account(&channel_key).expect("Missing channel");
    let (deposit, settled, _, stored_hash, status) = read_channel_state(updated.data());
    assert_eq!(deposit, 1_000_000);
    assert_eq!(settled, 0);
    assert_eq!(stored_hash, hash);
    assert_eq!(status, 0);
}

pub fn test_open_then_finalize(program_id: &Pubkey, binary_path: &str) {
    let mut mollusk = Mollusk::new(program_id, binary_path);

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = signer_account(operator);
    let (channel_key, channel_account) =
        empty_channel(&mut mollusk, program_id, operator, "channel-1");

    let recipient = Pubkey::new_unique();
    let hash = distribution_hash(&recipient, &[]);

    let open_ix: Instruction = OpenInstruction {
        operator: operator_key,
        channel: channel_key,
        deposit: 5_000_000,
        distribution_hash: hash,
    }
    .into();

    let finalize_ix: Instruction = FinalizeInstruction {
        operator: operator_key,
        channel: channel_key,
        new_settled: 3_000_000,
    }
    .into();

    let result = mollusk.process_and_validate_instruction_chain(
        &[
            (&open_ix, &[Check::success()]),
            (&finalize_ix, &[Check::success()]),
        ],
        &vec![
            (operator_key, operator_account),
            (channel_key, channel_account),
        ],
    );

    let updated = result.get_account(&channel_key).expect("Missing channel");
    let (deposit, settled, _, stored_hash, status) = read_channel_state(updated.data());
    assert_eq!(deposit, 5_000_000);
    assert_eq!(settled, 3_000_000);
    assert_eq!(stored_hash, hash);
    assert_eq!(status, 1);
}

pub fn test_distribute_no_splits(program_id: &Pubkey, binary_path: &str) {
    let mut mollusk = Mollusk::new(program_id, binary_path);

    let operator = Pubkey::new_unique();
    let sys = &solana_sdk_ids::system_program::ID;
    let recipient_key = Pubkey::new_unique();
    let payer_key = Pubkey::new_unique();
    let hash = distribution_hash(&recipient_key, &[]);

    let deposit = 5_000_000u64;
    let settled = 3_000_000u64;

    let (channel_key, channel_account) = finalized_channel(
        &mut mollusk,
        program_id,
        operator,
        "dist-0",
        deposit,
        settled,
        hash,
        deposit,
    );

    let dist_ix: Instruction = DistributeInstruction {
        channel: channel_key,
        recipient: recipient_key,
        payer: payer_key,
        splits: vec![],
    }
    .into();

    let result = mollusk.process_and_validate_instruction(
        &dist_ix,
        &[
            (channel_key, channel_account.clone()),
            (recipient_key, Account::new(0, 0, sys)),
            (payer_key, Account::new(0, 0, sys)),
        ],
        &[Check::success()],
    );

    assert_eq!(result.get_account(&channel_key).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&channel_key).unwrap().data()[40], 2);
    assert_eq!(
        result.get_account(&recipient_key).unwrap().lamports(),
        settled
    );

    let rent = channel_account.lamports - deposit;
    assert_eq!(
        result.get_account(&payer_key).unwrap().lamports(),
        (deposit - settled) + rent
    );
}

pub fn test_distribute_with_splits(program_id: &Pubkey, binary_path: &str) {
    let mut mollusk = Mollusk::new(program_id, binary_path);

    let operator = Pubkey::new_unique();
    let sys = &solana_sdk_ids::system_program::ID;
    let recipient_key = Pubkey::new_unique();
    let payer_key = Pubkey::new_unique();
    let split_a_key = Pubkey::new_unique();
    let split_b_key = Pubkey::new_unique();

    let splits = vec![
        Split {
            recipient: split_a_key,
            amount: 100_000,
        },
        Split {
            recipient: split_b_key,
            amount: 50_000,
        },
    ];
    let hash = distribution_hash(&recipient_key, &splits);

    let deposit = 10_000_000u64;
    let settled = 5_000_000u64;

    let (channel_key, channel_account) = finalized_channel(
        &mut mollusk,
        program_id,
        operator,
        "dist-1",
        deposit,
        settled,
        hash,
        deposit,
    );

    let dist_ix: Instruction = DistributeInstruction {
        channel: channel_key,
        recipient: recipient_key,
        payer: payer_key,
        splits: splits.clone(),
    }
    .into();

    let result = mollusk.process_and_validate_instruction(
        &dist_ix,
        &[
            (channel_key, channel_account.clone()),
            (recipient_key, Account::new(0, 0, sys)),
            (payer_key, Account::new(0, 0, sys)),
            (split_a_key, Account::new(0, 0, sys)),
            (split_b_key, Account::new(0, 0, sys)),
        ],
        &[Check::success()],
    );

    assert_eq!(result.get_account(&channel_key).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&channel_key).unwrap().data()[40], 2);
    assert_eq!(
        result.get_account(&split_a_key).unwrap().lamports(),
        100_000
    );
    assert_eq!(result.get_account(&split_b_key).unwrap().lamports(), 50_000);
    assert_eq!(
        result.get_account(&recipient_key).unwrap().lamports(),
        4_850_000
    );

    let rent = channel_account.lamports - deposit;
    assert_eq!(
        result.get_account(&payer_key).unwrap().lamports(),
        (deposit - settled) + rent
    );
}
