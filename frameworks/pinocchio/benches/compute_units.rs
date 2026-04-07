use fiber_sdk::{distribution_hash, Split, CHANNEL_DATA_SIZE};
use mollusk_svm::Mollusk;
use mollusk_svm_bencher::MolluskComputeUnitBencher;
use solana_account::Account;
use solana_clock::Epoch;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

const PROG_ID: Pubkey = Pubkey::new_from_array([0u8; 32]);

fn signer(key: Pubkey) -> (Pubkey, Account) {
    (
        key,
        Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
    )
}
fn open_ch(m: &mut Mollusk, b: Pubkey, s: &str, dep: u64, h: [u8; 16]) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&b, s, &PROG_ID).unwrap();
    let l = m.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    let mut d = vec![0u8; CHANNEL_DATA_SIZE];
    d[0..8].copy_from_slice(&dep.to_le_bytes());
    d[24..40].copy_from_slice(&h);
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
    let r = m.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    let mut d = vec![0u8; CHANNEL_DATA_SIZE];
    d[0..8].copy_from_slice(&dep.to_le_bytes());
    d[8..16].copy_from_slice(&set.to_le_bytes());
    d[24..40].copy_from_slice(&h);
    d[40] = 1;
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
fn empty_ch(m: &mut Mollusk, b: Pubkey, s: &str) -> (Pubkey, Account) {
    let k = Pubkey::create_with_seed(&b, s, &PROG_ID).unwrap();
    let l = m.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    (
        k,
        Account {
            lamports: l,
            data: vec![0u8; CHANNEL_DATA_SIZE],
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
    let settled = 1_000_000u64;
    let (ch, cha) = fin_ch(mollusk, op, seed, 5_000_000, settled, h);

    let mut data = vec![2u8]; // disc
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
    let mut mollusk = Mollusk::new(&PROG_ID, "../../target/deploy/fiber_pinocchio");
    let op = Pubkey::new_unique();
    let (op_k, op_a) = signer(op);
    let hash = distribution_hash(&Pubkey::new_unique(), &[]);

    let (c1, c1a) = empty_ch(&mut mollusk, op, "p-open");
    let mut p1 = vec![0u8];
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

    let (c2, c2a) = open_ch(&mut mollusk, op, "p-fin", 5_000_000, hash);
    let mut p2 = vec![1u8];
    p2.extend_from_slice(&3_000_000u64.to_le_bytes());
    let ix_fin = Instruction {
        program_id: PROG_ID,
        accounts: vec![
            AccountMeta::new_readonly(op_k, true),
            AccountMeta::new(c2, false),
        ],
        data: p2,
    };

    let (ix_d0, a_d0) = build_distribute(0, "p-d0", &mut mollusk, op);
    let (ix_d2, a_d2) = build_distribute(2, "p-d2", &mut mollusk, op);
    let (ix_d32, a_d32) = build_distribute(32, "p-d32", &mut mollusk, op);
    let (ix_d16, a_d16) = build_distribute(16, "p-d16", &mut mollusk, op);

    // --- Batch Finalize ---
    fn build_batch_finalize(
        n: usize,
        prefix: &str,
        mollusk: &mut Mollusk,
        op_k: Pubkey,
        op: Pubkey,
        hash: [u8; 16],
    ) -> (Instruction, Vec<(Pubkey, Account)>) {
        let mut data = vec![3u8]; // disc
        let mut accounts = vec![AccountMeta::new_readonly(op_k, true)];
        let mut accts = vec![(
            op_k,
            Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
        )];
        for i in 0..n {
            let (k, a) = open_ch(mollusk, op, &format!("{}-{}", prefix, i), 10_000_000, hash);
            data.extend_from_slice(&((i as u64 + 1) * 100_000).to_le_bytes());
            accounts.push(AccountMeta::new(k, false));
            accts.push((k, a));
        }
        (
            Instruction {
                program_id: PROG_ID,
                accounts,
                data,
            },
            accts,
        )
    }

    fn build_batch_open(
        n: usize,
        prefix: &str,
        mollusk: &mut Mollusk,
        op_k: Pubkey,
        op: Pubkey,
    ) -> (Instruction, Vec<(Pubkey, Account)>) {
        let mut data = vec![4u8]; // disc
        let mut accounts = vec![AccountMeta::new_readonly(op_k, true)];
        let mut accts = vec![(
            op_k,
            Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
        )];
        for i in 0..n {
            let (k, a) = empty_ch(mollusk, op, &format!("{}-{}", prefix, i));
            let h = distribution_hash(&Pubkey::new_unique(), &[]);
            data.extend_from_slice(&((i as u64 + 1) * 100_000).to_le_bytes());
            data.extend_from_slice(&h);
            accounts.push(AccountMeta::new(k, false));
            accts.push((k, a));
        }
        (
            Instruction {
                program_id: PROG_ID,
                accounts,
                data,
            },
            accts,
        )
    }

    let (ix_bf5, a_bf5) = build_batch_finalize(5, "pbf5", &mut mollusk, op_k, op, hash);
    let (ix_bf10, a_bf10) = build_batch_finalize(10, "pbf10", &mut mollusk, op_k, op, hash);
    let (ix_bf63, a_bf63) = build_batch_finalize(63, "pbf63", &mut mollusk, op_k, op, hash);
    let (ix_bo5, a_bo5) = build_batch_open(5, "pbo5", &mut mollusk, op_k, op);
    let (ix_bo63, a_bo63) = build_batch_open(63, "pbo63", &mut mollusk, op_k, op);

    MolluskComputeUnitBencher::new(mollusk)
        .bench((
            "Pinocchio Open",
            &ix_open,
            &[(op_k, op_a.clone()), (c1, c1a)],
        ))
        .bench(("Pinocchio Finalize", &ix_fin, &[(op_k, op_a), (c2, c2a)]))
        .bench(("Pinocchio Dist 0sp", &ix_d0, &a_d0))
        .bench(("Pinocchio Dist 2sp", &ix_d2, &a_d2))
        .bench(("Pinocchio Dist 16sp", &ix_d16, &a_d16))
        .bench(("Pinocchio Dist 32sp", &ix_d32, &a_d32))
        .bench(("Pinocchio BatchFin 5", &ix_bf5, &a_bf5))
        .bench(("Pinocchio BatchFin 10", &ix_bf10, &a_bf10))
        .bench(("Pinocchio BatchFin 63", &ix_bf63, &a_bf63))
        .bench(("Pinocchio BatchOpen 5", &ix_bo5, &a_bo5))
        .bench(("Pinocchio BatchOpen 63", &ix_bo63, &a_bo63))
        .must_pass(true)
        .out_dir("benches/")
        .execute();
}
