use fiber_sdk::{
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

fn keyed_account_for_signer(key: Pubkey) -> (Pubkey, Account) {
    (
        key,
        Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
    )
}

fn keyed_account_for_channel(
    mollusk: &mut Mollusk,
    operator: Pubkey,
    seed: &str,
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&operator, seed, &fiber_sdk::ID).unwrap();

    let lamports = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);

    let account = Account {
        lamports,
        data: vec![0u8; CHANNEL_DATA_SIZE],
        owner: fiber_sdk::ID,
        executable: false,
        rent_epoch: Epoch::default(),
    };

    (key, account)
}

/// Build a finalized channel account directly (for distribute-only tests)
fn keyed_account_for_finalized_channel(
    mollusk: &mut Mollusk,
    operator: Pubkey,
    seed: &str,
    deposit: u64,
    settled: u64,
    hash: [u8; 16],
    extra_lamports: u64,
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&operator, seed, &fiber_sdk::ID).unwrap();

    let rent = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);

    let mut data = vec![0u8; CHANNEL_DATA_SIZE];
    data[0..8].copy_from_slice(&deposit.to_le_bytes());
    data[8..16].copy_from_slice(&settled.to_le_bytes());
    // close_requested_at = 0 (already zeroed)
    data[24..40].copy_from_slice(&hash);
    data[40] = 1; // Finalized

    let account = Account {
        lamports: rent + extra_lamports,
        data,
        owner: fiber_sdk::ID,
        executable: false,
        rent_epoch: Epoch::default(),
    };

    (key, account)
}

fn writable_account(key: Pubkey) -> (Pubkey, Account) {
    (key, Account::new(0, 0, &solana_sdk_ids::system_program::ID))
}

fn read_channel_state(data: &[u8]) -> (u64, u64, i64, [u8; 16], u8) {
    let deposit = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let settled = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let close_req = i64::from_le_bytes(data[16..24].try_into().unwrap());
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&data[24..40]);
    let status = data[40];
    (deposit, settled, close_req, hash, status)
}

// === Open Tests ===

#[test]
fn test_open_channel() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = keyed_account_for_signer(operator);
    let (channel_key, channel_account) =
        keyed_account_for_channel(&mut mollusk, operator, "channel-0");

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
    let (deposit, settled, close_req, stored_hash, status) = read_channel_state(updated.data());

    assert_eq!(deposit, 1_000_000);
    assert_eq!(settled, 0);
    assert_eq!(close_req, 0);
    assert_eq!(stored_hash, hash);
    assert_eq!(status, 0); // Open
}

// === Finalize Tests ===

#[test]
fn test_open_then_finalize() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = keyed_account_for_signer(operator);
    let (channel_key, channel_account) =
        keyed_account_for_channel(&mut mollusk, operator, "channel-1");

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
    assert_eq!(status, 1); // Finalized
}

#[test]
fn test_finalize_rejects_over_deposit() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = keyed_account_for_signer(operator);
    let (channel_key, channel_account) =
        keyed_account_for_channel(&mut mollusk, operator, "channel-2");

    let hash = distribution_hash(&Pubkey::new_unique(), &[]);

    let open_ix: Instruction = OpenInstruction {
        operator: operator_key,
        channel: channel_key,
        deposit: 1_000_000,
        distribution_hash: hash,
    }
    .into();

    let finalize_ix: Instruction = FinalizeInstruction {
        operator: operator_key,
        channel: channel_key,
        new_settled: 2_000_000,
    }
    .into();

    mollusk.process_and_validate_instruction_chain(
        &[
            (&open_ix, &[Check::success()]),
            (
                &finalize_ix,
                &[Check::instruction_err(
                    solana_instruction::error::InstructionError::ProgramFailedToComplete,
                )],
            ),
        ],
        &vec![
            (operator_key, operator_account),
            (channel_key, channel_account),
        ],
    );
}

// === Distribute Tests ===

#[test]
fn test_distribute_no_splits() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let recipient_key = Pubkey::new_unique();
    let payer_key = Pubkey::new_unique();

    let hash = distribution_hash(&recipient_key, &[]);

    let deposit = 5_000_000u64;
    let settled = 3_000_000u64;

    let (channel_key, channel_account) = keyed_account_for_finalized_channel(
        &mut mollusk,
        operator,
        "dist-0",
        deposit,
        settled,
        hash,
        deposit, // extra lamports = deposit (simulates funded escrow)
    );
    let (recipient, recipient_account) = writable_account(recipient_key);
    let (payer, payer_account) = writable_account(payer_key);

    let dist_ix: Instruction = DistributeInstruction {
        channel: channel_key,
        recipient,
        payer,
        splits: vec![],
    }
    .into();

    let result = mollusk.process_and_validate_instruction(
        &dist_ix,
        &[
            (channel_key, channel_account.clone()),
            (recipient, recipient_account),
            (payer, payer_account),
        ],
        &[Check::success()],
    );

    let updated_channel = result.get_account(&channel_key).expect("channel");
    let updated_recipient = result.get_account(&recipient).expect("recipient");
    let updated_payer = result.get_account(&payer).expect("payer");

    // Channel should be closed with 0 lamports
    assert_eq!(updated_channel.lamports(), 0);
    assert_eq!(updated_channel.data()[40], 2); // Closed

    // Recipient gets settled amount
    assert_eq!(updated_recipient.lamports(), settled);

    // Payer gets refund (deposit - settled) + rent
    let rent = channel_account.lamports - deposit;
    assert_eq!(updated_payer.lamports(), (deposit - settled) + rent);
}

#[test]
fn test_distribute_with_splits() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
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

    let (channel_key, channel_account) = keyed_account_for_finalized_channel(
        &mut mollusk,
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
            (
                recipient_key,
                Account::new(0, 0, &solana_sdk_ids::system_program::ID),
            ),
            (
                payer_key,
                Account::new(0, 0, &solana_sdk_ids::system_program::ID),
            ),
            (
                split_a_key,
                Account::new(0, 0, &solana_sdk_ids::system_program::ID),
            ),
            (
                split_b_key,
                Account::new(0, 0, &solana_sdk_ids::system_program::ID),
            ),
        ],
        &[Check::success()],
    );

    let updated_channel = result.get_account(&channel_key).expect("channel");
    let updated_recipient = result.get_account(&recipient_key).expect("recipient");
    let updated_payer = result.get_account(&payer_key).expect("payer");
    let updated_split_a = result.get_account(&split_a_key).expect("split_a");
    let updated_split_b = result.get_account(&split_b_key).expect("split_b");

    assert_eq!(updated_channel.lamports(), 0);
    assert_eq!(updated_channel.data()[40], 2); // Closed

    // Splits get their fixed amounts
    assert_eq!(updated_split_a.lamports(), 100_000);
    assert_eq!(updated_split_b.lamports(), 50_000);

    // Recipient gets settled - sum(splits) = 5_000_000 - 150_000 = 4_850_000
    assert_eq!(updated_recipient.lamports(), 4_850_000);

    // Payer gets refund + rent
    let rent = channel_account.lamports - deposit;
    assert_eq!(updated_payer.lamports(), (deposit - settled) + rent);
}

#[test]
fn test_distribute_rejects_wrong_hash() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let recipient_key = Pubkey::new_unique();
    let payer_key = Pubkey::new_unique();
    let wrong_recipient = Pubkey::new_unique(); // different from hash

    let hash = distribution_hash(&recipient_key, &[]);

    let (channel_key, channel_account) = keyed_account_for_finalized_channel(
        &mut mollusk,
        operator,
        "dist-2",
        1_000_000,
        500_000,
        hash,
        1_000_000,
    );

    // Use wrong recipient — hash won't match
    let dist_ix: Instruction = DistributeInstruction {
        channel: channel_key,
        recipient: wrong_recipient,
        payer: payer_key,
        splits: vec![],
    }
    .into();

    mollusk.process_and_validate_instruction(
        &dist_ix,
        &[
            (channel_key, channel_account),
            (
                wrong_recipient,
                Account::new(0, 0, &solana_sdk_ids::system_program::ID),
            ),
            (
                payer_key,
                Account::new(0, 0, &solana_sdk_ids::system_program::ID),
            ),
        ],
        &[Check::instruction_err(
            solana_instruction::error::InstructionError::ProgramFailedToComplete,
        )],
    );
}

// === Batch Finalize Tests ===

#[test]
fn test_batch_finalize_3_channels() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = keyed_account_for_signer(operator);

    let hash = distribution_hash(&Pubkey::new_unique(), &[]);

    // Create 3 open channels with different deposits
    let deposits = [1_000_000u64, 2_000_000, 3_000_000];
    let settleds = [500_000u64, 1_500_000, 2_500_000];

    let mut channel_keys = Vec::new();
    let mut channel_accounts = Vec::new();

    for i in 0..3 {
        let seed = format!("batch-fin-{}", i);
        let key = Pubkey::create_with_seed(&operator, &seed, &fiber_sdk::ID).unwrap();
        let rent = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
        let mut data = vec![0u8; CHANNEL_DATA_SIZE];
        data[0..8].copy_from_slice(&deposits[i].to_le_bytes());
        data[24..40].copy_from_slice(&hash);
        // status = 0 (Open)
        let account = Account {
            lamports: rent,
            data,
            owner: fiber_sdk::ID,
            executable: false,
            rent_epoch: Epoch::default(),
        };
        channel_keys.push(key);
        channel_accounts.push(account);
    }

    let batch_ix: Instruction = BatchFinalizeInstruction {
        operator: operator_key,
        channels: vec![
            (channel_keys[0], settleds[0]),
            (channel_keys[1], settleds[1]),
            (channel_keys[2], settleds[2]),
        ],
    }
    .into();

    let mut accounts_vec: Vec<(Pubkey, Account)> = vec![(operator_key, operator_account)];
    for i in 0..3 {
        accounts_vec.push((channel_keys[i], channel_accounts[i].clone()));
    }

    let result =
        mollusk.process_and_validate_instruction(&batch_ix, &accounts_vec, &[Check::success()]);

    for i in 0..3 {
        let updated = result.get_account(&channel_keys[i]).expect("channel");
        let (deposit, settled, _, _, status) = read_channel_state(updated.data());
        assert_eq!(deposit, deposits[i], "Deposit unchanged for channel {}", i);
        assert_eq!(settled, settleds[i], "Settled updated for channel {}", i);
        assert_eq!(status, 1, "Status should be Finalized for channel {}", i);
    }
}

// === Batch Distribute Tests ===

#[test]
fn test_batch_distribute_1_channel() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");
    let operator = Pubkey::new_unique();
    let sys = &solana_sdk_ids::system_program::ID;

    let r = Pubkey::new_unique();
    let p = Pubkey::new_unique();
    let h = distribution_hash(&r, &[]);
    let (ch, cha) = keyed_account_for_finalized_channel(
        &mut mollusk,
        operator,
        "bd1-0",
        5_000_000,
        3_000_000,
        h,
        5_000_000,
    );

    let batch_ix: Instruction = BatchDistributeInstruction {
        entries: vec![DistributeEntry {
            channel: ch,
            recipient: r,
            payer: p,
            splits: vec![],
        }],
    }
    .into();

    mollusk.process_and_validate_instruction(
        &batch_ix,
        &[
            (ch, cha),
            (r, Account::new(0, 0, sys)),
            (p, Account::new(0, 0, sys)),
        ],
        &[Check::success()],
    );
}

#[test]
fn test_batch_distribute_3_channels() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let sys = &solana_sdk_ids::system_program::ID;

    // Channel 0: no splits, 5M deposit, 3M settled
    let r0 = Pubkey::new_unique();
    let p0 = Pubkey::new_unique();
    let h0 = distribution_hash(&r0, &[]);
    let (ch0, ch0a) = keyed_account_for_finalized_channel(
        &mut mollusk,
        operator,
        "bd-0",
        5_000_000,
        3_000_000,
        h0,
        5_000_000,
    );

    // Channel 1: 1 split, 10M deposit, 8M settled
    let r1 = Pubkey::new_unique();
    let p1 = Pubkey::new_unique();
    let s1 = Pubkey::new_unique();
    let splits1 = vec![Split {
        recipient: s1,
        amount: 200_000,
    }];
    let h1 = distribution_hash(&r1, &splits1);
    let (ch1, ch1a) = keyed_account_for_finalized_channel(
        &mut mollusk,
        operator,
        "bd-1",
        10_000_000,
        8_000_000,
        h1,
        10_000_000,
    );

    // Channel 2: no splits, 1M deposit, 1M settled (fully consumed)
    let r2 = Pubkey::new_unique();
    let p2 = Pubkey::new_unique();
    let h2 = distribution_hash(&r2, &[]);
    let (ch2, ch2a) = keyed_account_for_finalized_channel(
        &mut mollusk,
        operator,
        "bd-2",
        1_000_000,
        1_000_000,
        h2,
        1_000_000,
    );

    let batch_ix: Instruction = BatchDistributeInstruction {
        entries: vec![
            DistributeEntry {
                channel: ch0,
                recipient: r0,
                payer: p0,
                splits: vec![],
            },
            DistributeEntry {
                channel: ch1,
                recipient: r1,
                payer: p1,
                splits: splits1,
            },
            DistributeEntry {
                channel: ch2,
                recipient: r2,
                payer: p2,
                splits: vec![],
            },
        ],
    }
    .into();

    let rent = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);

    let result = mollusk.process_and_validate_instruction(
        &batch_ix,
        &[
            // Channel 0 group
            (ch0, ch0a),
            (r0, Account::new(0, 0, sys)),
            (p0, Account::new(0, 0, sys)),
            // Channel 1 group
            (ch1, ch1a),
            (r1, Account::new(0, 0, sys)),
            (p1, Account::new(0, 0, sys)),
            (s1, Account::new(0, 0, sys)),
            // Channel 2 group
            (ch2, ch2a),
            (r2, Account::new(0, 0, sys)),
            (p2, Account::new(0, 0, sys)),
        ],
        &[Check::success()],
    );

    // Channel 0: recipient gets 3M, payer gets 2M + rent
    assert_eq!(result.get_account(&ch0).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&r0).unwrap().lamports(), 3_000_000);
    assert_eq!(
        result.get_account(&p0).unwrap().lamports(),
        2_000_000 + rent
    );

    // Channel 1: split gets 200K, recipient gets 7.8M, payer gets 2M + rent
    assert_eq!(result.get_account(&ch1).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&s1).unwrap().lamports(), 200_000);
    assert_eq!(result.get_account(&r1).unwrap().lamports(), 7_800_000);
    assert_eq!(
        result.get_account(&p1).unwrap().lamports(),
        2_000_000 + rent
    );

    // Channel 2: recipient gets 1M, payer gets 0 + rent (fully consumed)
    assert_eq!(result.get_account(&ch2).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&r2).unwrap().lamports(), 1_000_000);
    assert_eq!(result.get_account(&p2).unwrap().lamports(), rent);

    // All channels closed
    assert_eq!(result.get_account(&ch0).unwrap().data()[40], 2);
    assert_eq!(result.get_account(&ch1).unwrap().data()[40], 2);
    assert_eq!(result.get_account(&ch2).unwrap().data()[40], 2);
}

// === Batch Open Tests ===

#[test]
fn test_batch_open_5_channels() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");

    let operator = Pubkey::new_unique();
    let (operator_key, operator_account) = keyed_account_for_signer(operator);

    let n = 5;
    let deposits: Vec<u64> = (0..n).map(|i| (i as u64 + 1) * 1_000_000).collect();
    let recipients: Vec<Pubkey> = (0..n).map(|_| Pubkey::new_unique()).collect();
    let hashes: Vec<[u8; 16]> = recipients
        .iter()
        .map(|r| distribution_hash(r, &[]))
        .collect();

    let mut channel_keys = Vec::new();
    let mut channel_accounts = Vec::new();
    for i in 0..n {
        let (key, account) =
            keyed_account_for_channel(&mut mollusk, operator, &format!("batch-open-{}", i));
        channel_keys.push(key);
        channel_accounts.push(account);
    }

    let channels: Vec<(Pubkey, u64, [u8; 16])> = (0..n)
        .map(|i| (channel_keys[i], deposits[i], hashes[i]))
        .collect();

    let batch_ix: Instruction = BatchOpenInstruction {
        operator: operator_key,
        channels,
    }
    .into();

    let mut accounts_vec: Vec<(Pubkey, Account)> = vec![(operator_key, operator_account)];
    for i in 0..n {
        accounts_vec.push((channel_keys[i], channel_accounts[i].clone()));
    }

    let result =
        mollusk.process_and_validate_instruction(&batch_ix, &accounts_vec, &[Check::success()]);

    for i in 0..n {
        let updated = result.get_account(&channel_keys[i]).expect("channel");
        let (deposit, settled, _, stored_hash, status) = read_channel_state(updated.data());
        assert_eq!(deposit, deposits[i]);
        assert_eq!(settled, 0);
        assert_eq!(stored_hash, hashes[i]);
        assert_eq!(status, 0);
    }
}
