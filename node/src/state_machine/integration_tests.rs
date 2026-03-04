use tempfile::NamedTempFile;

use crate::state_machine::{BankCommand, StateMachine};
use crate::wal::entry::LogEntry;
use crate::wal::wal::Wal;
use bank_api::bank::TransferStatus;

// ---- Helpers ----------------------------------------------------------------

fn append_cmd(wal: &mut Wal, index: u64, cmd: &BankCommand) {
    wal.append(LogEntry {
        index,
        term: 0,
        command: cmd.encode_to_bytes(),
    })
    .unwrap();
}

fn restore(wal: &Wal) -> StateMachine {
    StateMachine::restore(wal.replay().unwrap()).unwrap()
}

// ---- Tests ------------------------------------------------------------------

/// Commands written to the WAL produce the same state as applying them
/// directly, confirming the encode → persist → replay → decode round-trip.
#[test]
fn restore_matches_live_apply() {
    let tmp = NamedTempFile::new().unwrap();
    let mut wal = Wal::new(tmp.path().to_str().unwrap()).unwrap();

    let cmds = vec![
        BankCommand::CreateAccount { account_id: "alice".into(), initial_balance: 1000 },
        BankCommand::CreateAccount { account_id: "bob".into(), initial_balance: 0 },
        BankCommand::Transfer { from: "alice".into(), to: "bob".into(), amount: 400, client_tx_id: "tx-1".into() },
    ];

    // Build the live state machine.
    let mut live = StateMachine::new();
    for (i, cmd) in cmds.iter().enumerate() {
        append_cmd(&mut wal, (i + 1) as u64, cmd);
        live.apply(cmd.clone());
    }

    // Restore from WAL and compare.
    let restored = restore(&wal);
    assert_eq!(restored.get_balance("alice"), live.get_balance("alice"));
    assert_eq!(restored.get_balance("bob"), live.get_balance("bob"));
    assert_eq!(
        restored.get_transfer_status("tx-1"),
        live.get_transfer_status("tx-1")
    );
}

/// A new process opening an existing WAL file recovers full account state.
#[test]
fn crash_recovery_restores_balances() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    // "First process" — write and close.
    {
        let mut wal = Wal::new(path).unwrap();
        append_cmd(&mut wal, 1, &BankCommand::CreateAccount { account_id: "alice".into(), initial_balance: 500 });
        append_cmd(&mut wal, 2, &BankCommand::CreateAccount { account_id: "bob".into(), initial_balance: 200 });
        append_cmd(&mut wal, 3, &BankCommand::Transfer { from: "alice".into(), to: "bob".into(), amount: 100, client_tx_id: "tx-crash".into() });
    }

    // "Second process" — open same file, restore.
    let wal = Wal::new(path).unwrap();
    let sm = restore(&wal);

    assert_eq!(sm.get_balance("alice"), Some(400));
    assert_eq!(sm.get_balance("bob"), Some(300));
    assert_eq!(sm.get_transfer_status("tx-crash"), Some(TransferStatus::CommittedOk));
}

/// Failed transfers are persisted and replayed as failures — idempotency
/// is preserved across a crash boundary.
#[test]
fn crash_recovery_preserves_failed_transfer_in_dedupe() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    {
        let mut wal = Wal::new(path).unwrap();
        append_cmd(&mut wal, 1, &BankCommand::CreateAccount { account_id: "alice".into(), initial_balance: 50 });
        append_cmd(&mut wal, 2, &BankCommand::CreateAccount { account_id: "bob".into(), initial_balance: 0 });
        // This transfer will fail (insufficient funds) but still gets written to the WAL.
        append_cmd(&mut wal, 3, &BankCommand::Transfer { from: "alice".into(), to: "bob".into(), amount: 999, client_tx_id: "tx-fail".into() });
    }

    let wal = Wal::new(path).unwrap();
    let sm = restore(&wal);

    // Balances unchanged.
    assert_eq!(sm.get_balance("alice"), Some(50));
    assert_eq!(sm.get_balance("bob"), Some(0));
    // Failure is cached — a retry would return the same outcome.
    assert_eq!(sm.get_transfer_status("tx-fail"), Some(TransferStatus::CommittedInsufficientFunds));
}

/// Appending more entries to an existing WAL and re-restoring produces
/// correct cumulative state — confirms incremental writes work.
#[test]
fn incremental_wal_appends_accumulate_correctly() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    // First batch.
    {
        let mut wal = Wal::new(path).unwrap();
        append_cmd(&mut wal, 1, &BankCommand::CreateAccount { account_id: "alice".into(), initial_balance: 1000 });
        append_cmd(&mut wal, 2, &BankCommand::CreateAccount { account_id: "bob".into(), initial_balance: 0 });
    }

    // Second batch — same file, continues from index 3.
    {
        let mut wal = Wal::new(path).unwrap();
        append_cmd(&mut wal, 3, &BankCommand::Transfer { from: "alice".into(), to: "bob".into(), amount: 300, client_tx_id: "tx-a".into() });
        append_cmd(&mut wal, 4, &BankCommand::Transfer { from: "alice".into(), to: "bob".into(), amount: 200, client_tx_id: "tx-b".into() });
    }

    let wal = Wal::new(path).unwrap();
    let sm = restore(&wal);

    assert_eq!(sm.get_balance("alice"), Some(500));
    assert_eq!(sm.get_balance("bob"), Some(500));
    assert_eq!(sm.get_transfer_status("tx-a"), Some(TransferStatus::CommittedOk));
    assert_eq!(sm.get_transfer_status("tx-b"), Some(TransferStatus::CommittedOk));
}

/// Replaying the same WAL twice produces an identical state machine — restore
/// is deterministic and purely a function of the log contents.
#[test]
fn restore_is_deterministic() {
    let tmp = NamedTempFile::new().unwrap();
    let mut wal = Wal::new(tmp.path().to_str().unwrap()).unwrap();

    append_cmd(&mut wal, 1, &BankCommand::CreateAccount { account_id: "alice".into(), initial_balance: 800 });
    append_cmd(&mut wal, 2, &BankCommand::Transfer { from: "alice".into(), to: "ghost".into(), amount: 100, client_tx_id: "tx-det".into() });

    let sm1 = restore(&wal);
    let sm2 = restore(&wal);

    assert_eq!(sm1.get_balance("alice"), sm2.get_balance("alice"));
    assert_eq!(sm1.get_transfer_status("tx-det"), sm2.get_transfer_status("tx-det"));
}

/// A WAL with only CreateAccount entries (no transfers) restores correctly.
#[test]
fn restore_with_no_transfers() {
    let tmp = NamedTempFile::new().unwrap();
    let mut wal = Wal::new(tmp.path().to_str().unwrap()).unwrap();

    append_cmd(&mut wal, 1, &BankCommand::CreateAccount { account_id: "alice".into(), initial_balance: 100 });
    append_cmd(&mut wal, 2, &BankCommand::CreateAccount { account_id: "bob".into(), initial_balance: 200 });

    let sm = restore(&wal);

    assert_eq!(sm.get_balance("alice"), Some(100));
    assert_eq!(sm.get_balance("bob"), Some(200));
}

/// Opening an empty WAL produces an empty state machine.
#[test]
fn restore_from_empty_wal() {
    let tmp = NamedTempFile::new().unwrap();
    let wal = Wal::new(tmp.path().to_str().unwrap()).unwrap();
    let sm = restore(&wal);
    assert_eq!(sm.get_balance("nobody"), None);
}


