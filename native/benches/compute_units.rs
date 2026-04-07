use fiber_sdk::{
    distribution_hash, BatchDistributeInstruction, BatchFinalizeInstruction, BatchOpenInstruction,
    DistributeEntry, DistributeInstruction, FinalizeInstruction, OpenInstruction, Split,
    CHANNEL_DATA_SIZE,
};
use mollusk_svm::Mollusk;
use mollusk_svm_bencher::MolluskComputeUnitBencher;
use solana_account::Account;
use solana_clock::Epoch;
use solana_instruction::Instruction;
use solana_pubkey::Pubkey;

fn signer_account(key: Pubkey) -> (Pubkey, Account) {
    (
        key,
        Account::new(10_000_000_000, 0, &solana_sdk_ids::system_program::ID),
    )
}

fn empty_channel(mollusk: &mut Mollusk, base: Pubkey, seed: &str) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&base, seed, &fiber_sdk::ID).unwrap();
    let lamports = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    (
        key,
        Account {
            lamports,
            data: vec![0u8; CHANNEL_DATA_SIZE],
            owner: fiber_sdk::ID,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

fn open_channel(
    mollusk: &mut Mollusk,
    base: Pubkey,
    seed: &str,
    deposit: u64,
    hash: [u8; 16],
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&base, seed, &fiber_sdk::ID).unwrap();
    let lamports = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    let mut data = vec![0u8; CHANNEL_DATA_SIZE];
    data[0..8].copy_from_slice(&deposit.to_le_bytes());
    data[24..40].copy_from_slice(&hash);
    (
        key,
        Account {
            lamports,
            data,
            owner: fiber_sdk::ID,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

fn finalized_channel(
    mollusk: &mut Mollusk,
    base: Pubkey,
    seed: &str,
    deposit: u64,
    settled: u64,
    hash: [u8; 16],
) -> (Pubkey, Account) {
    let key = Pubkey::create_with_seed(&base, seed, &fiber_sdk::ID).unwrap();
    let rent = mollusk.sysvars.rent.minimum_balance(CHANNEL_DATA_SIZE);
    let mut data = vec![0u8; CHANNEL_DATA_SIZE];
    data[0..8].copy_from_slice(&deposit.to_le_bytes());
    data[8..16].copy_from_slice(&settled.to_le_bytes());
    data[24..40].copy_from_slice(&hash);
    data[40] = 1; // Finalized
    (
        key,
        Account {
            lamports: rent + deposit,
            data,
            owner: fiber_sdk::ID,
            executable: false,
            rent_epoch: Epoch::default(),
        },
    )
}

fn main() {
    let mut mollusk = Mollusk::new(&fiber_sdk::ID, "../target/deploy/fiber_native");
    let sys = &solana_sdk_ids::system_program::ID;

    let operator = Pubkey::new_unique();
    let (op_key, op_acct) = signer_account(operator);
    let hash = distribution_hash(&Pubkey::new_unique(), &[]);

    // --- Single Open ---
    let (ch1, ch1a) = empty_channel(&mut mollusk, operator, "s-open");
    let open_ix: Instruction = OpenInstruction {
        operator: op_key,
        channel: ch1,
        deposit: 1_000_000,
        distribution_hash: hash,
    }
    .into();

    // --- Single Finalize ---
    let (ch2, ch2a) = open_channel(&mut mollusk, operator, "s-fin", 5_000_000, hash);
    let fin_ix: Instruction = FinalizeInstruction {
        operator: op_key,
        channel: ch2,
        new_settled: 3_000_000,
    }
    .into();

    // --- Distribute 0 splits ---
    let r0 = Pubkey::new_unique();
    let p0 = Pubkey::new_unique();
    let h0 = distribution_hash(&r0, &[]);
    let (ch3, ch3a) = finalized_channel(&mut mollusk, operator, "d0", 5_000_000, 3_000_000, h0);
    let dist0_ix: Instruction = DistributeInstruction {
        channel: ch3,
        recipient: r0,
        payer: p0,
        splits: vec![],
    }
    .into();

    // --- Distribute 4 splits ---
    let r4 = Pubkey::new_unique();
    let p4 = Pubkey::new_unique();
    let splits_4: Vec<Split> = (0..4)
        .map(|_| Split {
            recipient: Pubkey::new_unique(),
            amount: 100_000,
        })
        .collect();
    let h4 = distribution_hash(&r4, &splits_4);
    let (ch4, ch4a) = finalized_channel(&mut mollusk, operator, "d4", 10_000_000, 5_000_000, h4);
    let dist4_ix: Instruction = DistributeInstruction {
        channel: ch4,
        recipient: r4,
        payer: p4,
        splits: splits_4.clone(),
    }
    .into();

    // --- Distribute 2 splits ---
    let r2 = Pubkey::new_unique();
    let p2 = Pubkey::new_unique();
    let splits_2: Vec<Split> = (0..2)
        .map(|_| Split {
            recipient: Pubkey::new_unique(),
            amount: 50_000,
        })
        .collect();
    let h2 = distribution_hash(&r2, &splits_2);
    let (ch_d2, ch_d2a) = finalized_channel(&mut mollusk, operator, "d2", 5_000_000, 1_000_000, h2);
    let dist2_ix: Instruction = DistributeInstruction {
        channel: ch_d2,
        recipient: r2,
        payer: p2,
        splits: splits_2.clone(),
    }
    .into();

    // --- Distribute 16 splits ---
    let r16 = Pubkey::new_unique();
    let p16 = Pubkey::new_unique();
    let splits_16: Vec<Split> = (0..16)
        .map(|_| Split {
            recipient: Pubkey::new_unique(),
            amount: 10_000,
        })
        .collect();
    let h16 = distribution_hash(&r16, &splits_16);
    let (ch_d16, ch_d16a) =
        finalized_channel(&mut mollusk, operator, "d16", 5_000_000, 1_000_000, h16);
    let dist16_ix: Instruction = DistributeInstruction {
        channel: ch_d16,
        recipient: r16,
        payer: p16,
        splits: splits_16.clone(),
    }
    .into();

    // --- Distribute 32 splits ---
    let r32 = Pubkey::new_unique();
    let p32 = Pubkey::new_unique();
    let splits_32: Vec<Split> = (0..32)
        .map(|_| Split {
            recipient: Pubkey::new_unique(),
            amount: 5_000,
        })
        .collect();
    let h32 = distribution_hash(&r32, &splits_32);
    let (ch_d32, ch_d32a) =
        finalized_channel(&mut mollusk, operator, "d32", 5_000_000, 1_000_000, h32);
    let dist32_ix: Instruction = DistributeInstruction {
        channel: ch_d32,
        recipient: r32,
        payer: p32,
        splits: splits_32.clone(),
    }
    .into();

    // --- Batch Finalize (5 channels) ---
    let mut bf_channels = Vec::new();
    let mut bf_accounts: Vec<(Pubkey, Account)> = vec![(op_key, op_acct.clone())];
    for i in 0..5 {
        let (k, a) = open_channel(
            &mut mollusk,
            operator,
            &format!("bf-{}", i),
            5_000_000,
            hash,
        );
        bf_channels.push((k, (i as u64 + 1) * 500_000));
        bf_accounts.push((k, a));
    }
    let bf_ix: Instruction = BatchFinalizeInstruction {
        operator: op_key,
        channels: bf_channels,
    }
    .into();

    // --- Batch Finalize (10 channels) ---
    let mut bf10_channels = Vec::new();
    let mut bf10_accounts: Vec<(Pubkey, Account)> = vec![(op_key, op_acct.clone())];
    for i in 0..10 {
        let (k, a) = open_channel(
            &mut mollusk,
            operator,
            &format!("bf10-{}", i),
            10_000_000,
            hash,
        );
        bf10_channels.push((k, (i as u64 + 1) * 500_000));
        bf10_accounts.push((k, a));
    }
    let bf10_ix: Instruction = BatchFinalizeInstruction {
        operator: op_key,
        channels: bf10_channels,
    }
    .into();

    // --- Batch Finalize (63 channels — max accounts) ---
    let mut bf63_channels = Vec::new();
    let mut bf63_accounts: Vec<(Pubkey, Account)> = vec![(op_key, op_acct.clone())];
    for i in 0..63 {
        let (k, a) = open_channel(
            &mut mollusk,
            operator,
            &format!("bf63-{}", i),
            10_000_000,
            hash,
        );
        bf63_channels.push((k, (i as u64 + 1) * 100_000));
        bf63_accounts.push((k, a));
    }
    let bf63_ix: Instruction = BatchFinalizeInstruction {
        operator: op_key,
        channels: bf63_channels,
    }
    .into();

    // --- Batch Distribute (3 channels, 0 splits each) ---
    let mut bd_entries = Vec::new();
    let mut bd_accounts: Vec<(Pubkey, Account)> = Vec::new();
    for i in 0..3 {
        let r = Pubkey::new_unique();
        let p = Pubkey::new_unique();
        let h = distribution_hash(&r, &[]);
        let (k, a) = finalized_channel(
            &mut mollusk,
            operator,
            &format!("bd-{}", i),
            5_000_000,
            3_000_000,
            h,
        );
        bd_entries.push(DistributeEntry {
            channel: k,
            recipient: r,
            payer: p,
            splits: vec![],
        });
        bd_accounts.push((k, a));
        bd_accounts.push((r, Account::new(0, 0, sys)));
        bd_accounts.push((p, Account::new(0, 0, sys)));
    }
    let bd_ix: Instruction = BatchDistributeInstruction {
        entries: bd_entries,
    }
    .into();

    // --- Batch Open (5 + 63 channels) ---
    let mut bo5_channels = Vec::new();
    let mut bo5_accounts: Vec<(Pubkey, Account)> = vec![(op_key, op_acct.clone())];
    for i in 0..5 {
        let (k, a) = empty_channel(&mut mollusk, operator, &format!("bo5-{}", i));
        let h = distribution_hash(&Pubkey::new_unique(), &[]);
        bo5_channels.push((k, (i as u64 + 1) * 1_000_000, h));
        bo5_accounts.push((k, a));
    }
    let bo5_ix: Instruction = BatchOpenInstruction {
        operator: op_key,
        channels: bo5_channels,
    }
    .into();

    let mut bo63_channels = Vec::new();
    let mut bo63_accounts: Vec<(Pubkey, Account)> = vec![(op_key, op_acct.clone())];
    for i in 0..63 {
        let (k, a) = empty_channel(&mut mollusk, operator, &format!("bo63-{}", i));
        let h = distribution_hash(&Pubkey::new_unique(), &[]);
        bo63_channels.push((k, (i as u64 + 1) * 100_000, h));
        bo63_accounts.push((k, a));
    }
    let bo63_ix: Instruction = BatchOpenInstruction {
        operator: op_key,
        channels: bo63_channels,
    }
    .into();

    MolluskComputeUnitBencher::new(mollusk)
        .bench((
            "Open (single)",
            &open_ix,
            &[(op_key, op_acct.clone()), (ch1, ch1a)],
        ))
        .bench((
            "Finalize (single)",
            &fin_ix,
            &[(op_key, op_acct.clone()), (ch2, ch2a)],
        ))
        .bench((
            "Distribute (0 splits)",
            &dist0_ix,
            &[
                (ch3, ch3a),
                (r0, Account::new(0, 0, sys)),
                (p0, Account::new(0, 0, sys)),
            ],
        ))
        .bench((
            "Distribute (4 splits)",
            &dist4_ix,
            &[
                (ch4, ch4a),
                (r4, Account::new(0, 0, sys)),
                (p4, Account::new(0, 0, sys)),
                (splits_4[0].recipient, Account::new(0, 0, sys)),
                (splits_4[1].recipient, Account::new(0, 0, sys)),
                (splits_4[2].recipient, Account::new(0, 0, sys)),
                (splits_4[3].recipient, Account::new(0, 0, sys)),
            ],
        ))
        .bench(("Distribute (2 splits)", &dist2_ix, &{
            let mut a = vec![
                (ch_d2, ch_d2a),
                (r2, Account::new(0, 0, sys)),
                (p2, Account::new(0, 0, sys)),
            ];
            for s in &splits_2 {
                a.push((s.recipient, Account::new(0, 0, sys)));
            }
            a
        }))
        .bench(("Distribute (16 splits)", &dist16_ix, &{
            let mut a = vec![
                (ch_d16, ch_d16a),
                (r16, Account::new(0, 0, sys)),
                (p16, Account::new(0, 0, sys)),
            ];
            for s in &splits_16 {
                a.push((s.recipient, Account::new(0, 0, sys)));
            }
            a
        }))
        .bench(("Distribute (32 splits)", &dist32_ix, &{
            let mut a = vec![
                (ch_d32, ch_d32a),
                (r32, Account::new(0, 0, sys)),
                (p32, Account::new(0, 0, sys)),
            ];
            for s in &splits_32 {
                a.push((s.recipient, Account::new(0, 0, sys)));
            }
            a
        }))
        .bench(("BatchFinalize (5 ch)", &bf_ix, &bf_accounts))
        .bench(("BatchFinalize (10 ch)", &bf10_ix, &bf10_accounts))
        .bench(("BatchFinalize (63 ch)", &bf63_ix, &bf63_accounts))
        .bench(("BatchDistribute (3ch, 0sp)", &bd_ix, &bd_accounts))
        .bench(("BatchOpen (5 ch)", &bo5_ix, &bo5_accounts))
        .bench(("BatchOpen (63 ch)", &bo63_ix, &bo63_accounts))
        .must_pass(true)
        .out_dir("benches/")
        .execute();
}
