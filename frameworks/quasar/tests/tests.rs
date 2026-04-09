//! Quasar implementation tests — uses shared test helpers from fiber-sdk.

use fiber_sdk::test_helpers;

const PROGRAM_ID: solana_pubkey::Pubkey = solana_pubkey::Pubkey::new_from_array([0u8; 32]);
const BINARY: &str = "../../target/deploy/fiber_quasar";

#[test]
fn test_open() {
    test_helpers::test_open(&PROGRAM_ID, BINARY);
}

#[test]
fn test_open_then_finalize() {
    test_helpers::test_open_then_finalize(&PROGRAM_ID, BINARY);
}

#[test]
fn test_distribute_no_splits() {
    test_helpers::test_distribute_no_splits(&PROGRAM_ID, BINARY);
}

#[test]
fn test_distribute_with_splits() {
    test_helpers::test_distribute_with_splits(&PROGRAM_ID, BINARY);
}
