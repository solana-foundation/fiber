//! Anchor implementation tests.
//!
//! Anchor uses a different instruction format (8-byte discriminator hash +
//! borsh encoding) and 50-byte accounts (8-byte discriminator + 42 data),
//! so these tests can't share the helpers used by native/pinocchio/quasar.

use fiber_sdk::{distribution_hash, Split, CHANNEL_DATA_SIZE};
use mollusk_svm::result::Check;
use mollusk_svm::Mollusk;
use solana_account::{Account, ReadableAccount};
use solana_clock::Epoch;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

const PROG_ID: Pubkey = Pubkey::new_from_array([0u8; 32]);
const ANCHOR_SIZE: usize = 8 + CHANNEL_DATA_SIZE; // discriminator + data

fn anchor_ix_disc(name: &str) -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("global:{name}").as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

fn acct_disc() -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(b"account:Channel");
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

fn signer(key: Pubkey) -> (Pubkey, Account) {
    (
        key,
        Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
    )
}

fn empty_ch(m: &mut Mollusk, op: Pubkey, seed: &str) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&op, seed, &PROG_ID).unwrap();
    let l = m.sysvars.rent.minimum_balance(ANCHOR_SIZE);
    let mut d = vec![0u8; ANCHOR_SIZE];
    d[..8].copy_from_slice(&acct_disc());
    (
        k,
        Account {
            lamports: l,
            data: d,
            owner: PROG_ID,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

fn finalized_ch(
    m: &mut Mollusk,
    op: Pubkey,
    seed: &str,
    deposit: u64,
    settled: u64,
    hash: [u8; 16],
    extra: u64,
) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&op, seed, &PROG_ID).unwrap();
    let r = m.sysvars.rent.minimum_balance(ANCHOR_SIZE);
    let mut d = vec![0u8; ANCHOR_SIZE];
    d[..8].copy_from_slice(&acct_disc());
    d[8..16].copy_from_slice(&deposit.to_le_bytes());
    d[16..24].copy_from_slice(&settled.to_le_bytes());
    d[32..48].copy_from_slice(&hash);
    d[48] = 1; // Finalized
    (
        k,
        Account {
            lamports: r + extra,
            data: d,
            owner: PROG_ID,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

// Anchor data offsets (8 bytes after discriminator)
fn read_anchor_state(data: &[u8]) -> (u64, u64, [u8; 16], u8) {
    let deposit = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let settled = u64::from_le_bytes(data[16..24].try_into().unwrap());
    let mut hash = [0u8; 16];
    hash.copy_from_slice(&data[32..48]);
    let status = data[48];
    (deposit, settled, hash, status)
}

#[test]
fn test_open() {
    let mut mollusk = Mollusk::new(&PROG_ID, "../../target/deploy/fiber_anchor");
    let op = Pubkey::new_unique();
    let (op_k, op_a) = signer(op);
    let (ch, cha) = empty_ch(&mut mollusk, op, "a-open");

    let hash = distribution_hash(&Pubkey::new_unique(), &[]);
    let mut data = Vec::new();
    data.extend_from_slice(&anchor_ix_disc("open"));
    data.extend_from_slice(&1_000_000u64.to_le_bytes());
    data.extend_from_slice(&hash);

    let ix = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new_readonly(op_k, true),
            AccountMeta::new(ch, false),
        ],
        data,
    };

    let result = mollusk.process_and_validate_instruction(
        &ix,
        &[(op_k, op_a), (ch, cha)],
        &[Check::success()],
    );

    let updated = result.get_account(&ch).unwrap();
    let (deposit, settled, stored_hash, status) = read_anchor_state(updated.data());
    assert_eq!(deposit, 1_000_000);
    assert_eq!(settled, 0);
    assert_eq!(stored_hash, hash);
    assert_eq!(status, 0);
}

#[test]
fn test_open_then_finalize() {
    let mut mollusk = Mollusk::new(&PROG_ID, "../../target/deploy/fiber_anchor");
    let op = Pubkey::new_unique();
    let (op_k, op_a) = signer(op);
    let (ch, cha) = empty_ch(&mut mollusk, op, "a-fin");

    let hash = distribution_hash(&Pubkey::new_unique(), &[]);

    let mut open_data = Vec::new();
    open_data.extend_from_slice(&anchor_ix_disc("open"));
    open_data.extend_from_slice(&5_000_000u64.to_le_bytes());
    open_data.extend_from_slice(&hash);
    let open_ix = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new_readonly(op_k, true),
            AccountMeta::new(ch, false),
        ],
        data: open_data,
    };

    let mut fin_data = Vec::new();
    fin_data.extend_from_slice(&anchor_ix_disc("finalize"));
    fin_data.extend_from_slice(&3_000_000u64.to_le_bytes());
    let fin_ix = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new_readonly(op_k, true),
            AccountMeta::new(ch, false),
        ],
        data: fin_data,
    };

    let result = mollusk.process_and_validate_instruction_chain(
        &[
            (&open_ix, &[Check::success()]),
            (&fin_ix, &[Check::success()]),
        ],
        &vec![(op_k, op_a), (ch, cha)],
    );

    let updated = result.get_account(&ch).unwrap();
    let (deposit, settled, _, status) = read_anchor_state(updated.data());
    assert_eq!(deposit, 5_000_000);
    assert_eq!(settled, 3_000_000);
    assert_eq!(status, 1);
}

#[test]
fn test_distribute_no_splits() {
    let mut mollusk = Mollusk::new(&PROG_ID, "../../target/deploy/fiber_anchor");
    let op = Pubkey::new_unique();
    let sys = &solana_sdk_ids::system_program::ID;

    let r = Pubkey::new_unique();
    let p = Pubkey::new_unique();
    let hash = distribution_hash(&r, &[]);
    let deposit = 5_000_000u64;
    let settled = 3_000_000u64;

    let (ch, cha) = finalized_ch(&mut mollusk, op, "a-d0", deposit, settled, hash, deposit);

    let mut data = Vec::new();
    data.extend_from_slice(&anchor_ix_disc("distribute"));
    data.extend_from_slice(&0u32.to_le_bytes()); // borsh Vec len = 0

    let ix = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new(ch, false),
            AccountMeta::new(r, false),
            AccountMeta::new(p, false),
        ],
        data,
    };

    let result = mollusk.process_and_validate_instruction(
        &ix,
        &[
            (ch, cha.clone()),
            (r, Account::new(0, 0, sys)),
            (p, Account::new(0, 0, sys)),
        ],
        &[Check::success()],
    );

    assert_eq!(result.get_account(&ch).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&ch).unwrap().data()[48], 2); // Closed
    assert_eq!(result.get_account(&r).unwrap().lamports(), settled);

    let rent = cha.lamports - deposit;
    assert_eq!(
        result.get_account(&p).unwrap().lamports(),
        (deposit - settled) + rent
    );
}

#[test]
fn test_distribute_with_splits() {
    let mut mollusk = Mollusk::new(&PROG_ID, "../../target/deploy/fiber_anchor");
    let op = Pubkey::new_unique();
    let sys = &solana_sdk_ids::system_program::ID;

    let r = Pubkey::new_unique();
    let p = Pubkey::new_unique();
    let sa = Pubkey::new_unique();
    let sb = Pubkey::new_unique();

    let splits = vec![
        Split {
            recipient: sa,
            amount: 100_000,
        },
        Split {
            recipient: sb,
            amount: 50_000,
        },
    ];
    let hash = distribution_hash(&r, &splits);
    let deposit = 10_000_000u64;
    let settled = 5_000_000u64;

    let (ch, cha) = finalized_ch(&mut mollusk, op, "a-d2", deposit, settled, hash, deposit);

    let mut data = Vec::new();
    data.extend_from_slice(&anchor_ix_disc("distribute"));
    data.extend_from_slice(&2u32.to_le_bytes()); // borsh Vec len = 2
    data.extend_from_slice(&100_000u64.to_le_bytes());
    data.extend_from_slice(&50_000u64.to_le_bytes());

    let ix = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new(ch, false),
            AccountMeta::new(r, false),
            AccountMeta::new(p, false),
            AccountMeta::new(sa, false),
            AccountMeta::new(sb, false),
        ],
        data,
    };

    let result = mollusk.process_and_validate_instruction(
        &ix,
        &[
            (ch, cha.clone()),
            (r, Account::new(0, 0, sys)),
            (p, Account::new(0, 0, sys)),
            (sa, Account::new(0, 0, sys)),
            (sb, Account::new(0, 0, sys)),
        ],
        &[Check::success()],
    );

    assert_eq!(result.get_account(&ch).unwrap().lamports(), 0);
    assert_eq!(result.get_account(&ch).unwrap().data()[48], 2);
    assert_eq!(result.get_account(&sa).unwrap().lamports(), 100_000);
    assert_eq!(result.get_account(&sb).unwrap().lamports(), 50_000);
    assert_eq!(result.get_account(&r).unwrap().lamports(), 4_850_000);

    let rent = cha.lamports - deposit;
    assert_eq!(
        result.get_account(&p).unwrap().lamports(),
        (deposit - settled) + rent
    );
}
