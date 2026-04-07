use fiber_sdk::{distribution_hash, Split, CHANNEL_DATA_SIZE};
use mollusk_svm::Mollusk;
use mollusk_svm_bencher::MolluskComputeUnitBencher;
use solana_account::Account;
use solana_clock::Epoch;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

const PROG_ID: Pubkey = Pubkey::new_from_array([0u8; 32]);

// Anchor adds 8-byte discriminator before account data
const ANCHOR_CHANNEL_SIZE: usize = 8 + CHANNEL_DATA_SIZE;

fn anchor_disc(name: &str) -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("global:{name}").as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

fn account_disc() -> [u8; 8] {
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

fn empty_ch(m: &mut Mollusk, b: Pubkey, s: &str) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&b, s, &PROG_ID).unwrap();
    let l = m.sysvars.rent.minimum_balance(ANCHOR_CHANNEL_SIZE);
    let mut d = vec![0u8; ANCHOR_CHANNEL_SIZE];
    d[..8].copy_from_slice(&account_disc());
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

fn open_ch(m: &mut Mollusk, b: Pubkey, s: &str, dep: u64, h: [u8; 16]) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&b, s, &PROG_ID).unwrap();
    let l = m.sysvars.rent.minimum_balance(ANCHOR_CHANNEL_SIZE);
    let mut d = vec![0u8; ANCHOR_CHANNEL_SIZE];
    d[..8].copy_from_slice(&account_disc());
    d[8..16].copy_from_slice(&dep.to_le_bytes()); // deposit at +8
    d[32..48].copy_from_slice(&h); // hash at +8+24=32
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

fn fin_ch(
    m: &mut Mollusk,
    b: Pubkey,
    s: &str,
    dep: u64,
    set: u64,
    h: [u8; 16],
) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&b, s, &PROG_ID).unwrap();
    let r = m.sysvars.rent.minimum_balance(ANCHOR_CHANNEL_SIZE);
    let mut d = vec![0u8; ANCHOR_CHANNEL_SIZE];
    d[..8].copy_from_slice(&account_disc());
    d[8..16].copy_from_slice(&dep.to_le_bytes());
    d[16..24].copy_from_slice(&set.to_le_bytes()); // settled at +8+8=16
    d[32..48].copy_from_slice(&h); // hash at +8+24=32
    d[48] = 1; // status at +8+40=48 (Finalized)
    (
        k,
        Account {
            lamports: r + dep,
            data: d,
            owner: PROG_ID,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

fn build_distribute(
    n_splits: usize,
    seed: &str,
    mollusk: &mut Mollusk,
    op: Pubkey,
) -> (Instruction, Vec<(Pubkey, Account)>) {
    let sys = &solana_sdk_ids::system_program::ID;
    let r = Pubkey::new_unique();
    let p = Pubkey::new_unique();
    let splits: Vec<Split> = (0..n_splits)
        .map(|_| Split {
            recipient: Pubkey::new_unique(),
            amount: 10_000,
        })
        .collect();
    let h = distribution_hash(&r, &splits);
    let (ch, cha) = fin_ch(mollusk, op, seed, 5_000_000, 1_000_000, h);

    // Anchor: [8-byte ix disc, borsh Vec<u64> = (u32 len, u64...)]
    let mut data = Vec::new();
    data.extend_from_slice(&anchor_disc("distribute"));
    data.extend_from_slice(&(n_splits as u32).to_le_bytes());
    for s in &splits {
        data.extend_from_slice(&s.amount.to_le_bytes());
    }

    let mut accounts = vec![
        AccountMeta::new(ch, false),
        AccountMeta::new(r, false),
        AccountMeta::new(p, false),
    ];
    for s in &splits {
        accounts.push(AccountMeta::new(s.recipient, false));
    }
    let ix = Instruction {
        program_id: PROG_ID,
        accounts,
        data,
    };

    let mut accts = vec![
        (ch, cha),
        (r, Account::new(0, 0, sys)),
        (p, Account::new(0, 0, sys)),
    ];
    for s in &splits {
        accts.push((s.recipient, Account::new(0, 0, sys)));
    }
    (ix, accts)
}

fn main() {
    let mut mollusk = Mollusk::new(&PROG_ID, "../../target/deploy/fiber_anchor");
    let op = Pubkey::new_unique();
    let (op_k, op_a) = signer(op);
    let hash = distribution_hash(&Pubkey::new_unique(), &[]);

    let (c1, c1a) = empty_ch(&mut mollusk, op, "a-open");
    let mut p1 = Vec::new();
    p1.extend_from_slice(&anchor_disc("open"));
    p1.extend_from_slice(&1_000_000u64.to_le_bytes());
    p1.extend_from_slice(&hash);
    let ix_open = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new_readonly(op_k, true),
            AccountMeta::new(c1, false),
        ],
        data: p1,
    };

    let (c2, c2a) = open_ch(&mut mollusk, op, "a-fin", 5_000_000, hash);
    let mut p2 = Vec::new();
    p2.extend_from_slice(&anchor_disc("finalize"));
    p2.extend_from_slice(&3_000_000u64.to_le_bytes());
    let ix_fin = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new_readonly(op_k, true),
            AccountMeta::new(c2, false),
        ],
        data: p2,
    };

    let (ix_d0, a_d0) = build_distribute(0, "a-d0", &mut mollusk, op);
    let (ix_d2, a_d2) = build_distribute(2, "a-d2", &mut mollusk, op);
    let (ix_d16, a_d16) = build_distribute(16, "a-d16", &mut mollusk, op);
    let (ix_d32, a_d32) = build_distribute(32, "a-d32", &mut mollusk, op);

    MolluskComputeUnitBencher::new(mollusk)
        .bench(("Anchor Open", &ix_open, &[(op_k, op_a.clone()), (c1, c1a)]))
        .bench(("Anchor Finalize", &ix_fin, &[(op_k, op_a), (c2, c2a)]))
        .bench(("Anchor Dist 0sp", &ix_d0, &a_d0))
        .bench(("Anchor Dist 2sp", &ix_d2, &a_d2))
        .bench(("Anchor Dist 16sp", &ix_d16, &a_d16))
        .bench(("Anchor Dist 32sp", &ix_d32, &a_d32))
        .must_pass(true)
        .out_dir("benches/")
        .execute();
}
