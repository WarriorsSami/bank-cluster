use tempfile::NamedTempFile;

use crate::state_machine::{BankCommand, TransferResult};
use crate::store::store::Store;

// ---- Helpers ----------------------------------------------------------------

fn open_store(path: &str) -> Store {
    Store::open(path).unwrap()
}

// ---- Tests ------------------------------------------------------------------

/// Executing commands through the store produces the expected state.
#[test]
fn execute_create_and_transfer() {
    let tmp = NamedTempFile::new().unwrap();
    let mut store = open_store(tmp.path().to_str().unwrap());

    store
        .execute(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 1000,
        })
        .unwrap();
    store
        .execute(BankCommand::CreateAccount {
            account_id: "bob".into(),
            initial_balance: 0,
        })
        .unwrap();
    store
        .execute(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 400,
            client_tx_id: "tx-1".into(),
        })
        .unwrap();

    assert_eq!(store.get_balance("alice"), Some(600));
    assert_eq!(store.get_balance("bob"), Some(400));
    assert_eq!(store.get_transfer_status("tx-1"), Some(TransferResult::Ok));
}

/// After closing and reopening the store, all previously executed commands
/// are replayed from the WAL and state is fully recovered.
#[test]
fn crash_recovery_restores_state() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    // "First process" — execute commands and close.
    {
        let mut store = open_store(path);
        store
            .execute(BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 500,
            })
            .unwrap();
        store
            .execute(BankCommand::CreateAccount {
                account_id: "bob".into(),
                initial_balance: 200,
            })
            .unwrap();
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 100,
                client_tx_id: "tx-crash".into(),
            })
            .unwrap();
    }

    // "Second process" — reopen and verify recovered state.
    let store = open_store(path);
    assert_eq!(store.get_balance("alice"), Some(400));
    assert_eq!(store.get_balance("bob"), Some(300));
    assert_eq!(
        store.get_transfer_status("tx-crash"),
        Some(TransferResult::Ok)
    );
}

/// A failed transfer is written to the WAL and replayed as a failure —
/// idempotency is preserved across a crash boundary.
#[test]
fn crash_recovery_preserves_failed_transfer() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    {
        let mut store = open_store(path);
        store
            .execute(BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 50,
            })
            .unwrap();
        store
            .execute(BankCommand::CreateAccount {
                account_id: "bob".into(),
                initial_balance: 0,
            })
            .unwrap();
        // This transfer will fail (insufficient funds) but is still written to the WAL.
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 999,
                client_tx_id: "tx-fail".into(),
            })
            .unwrap();
    }

    let store = open_store(path);
    // Balances unchanged.
    assert_eq!(store.get_balance("alice"), Some(50));
    assert_eq!(store.get_balance("bob"), Some(0));
    // Failure is still cached after recovery.
    assert_eq!(
        store.get_transfer_status("tx-fail"),
        Some(TransferResult::InsufficientFunds)
    );
}

/// Executing commands across multiple store sessions accumulates state
/// correctly — confirms incremental WAL appends work end-to-end.
#[test]
fn incremental_sessions_accumulate_correctly() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    // First session: create accounts.
    {
        let mut store = open_store(path);
        store
            .execute(BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 1000,
            })
            .unwrap();
        store
            .execute(BankCommand::CreateAccount {
                account_id: "bob".into(),
                initial_balance: 0,
            })
            .unwrap();
    }

    // Second session: transfer in two steps.
    {
        let mut store = open_store(path);
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 300,
                client_tx_id: "tx-a".into(),
            })
            .unwrap();
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 200,
                client_tx_id: "tx-b".into(),
            })
            .unwrap();
    }

    let store = open_store(path);
    assert_eq!(store.get_balance("alice"), Some(500));
    assert_eq!(store.get_balance("bob"), Some(500));
    assert_eq!(store.get_transfer_status("tx-a"), Some(TransferResult::Ok));
    assert_eq!(store.get_transfer_status("tx-b"), Some(TransferResult::Ok));
}

/// Reopening the same WAL twice produces identical state — restore is
/// deterministic and purely a function of the log contents.
#[test]
fn restore_is_deterministic() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    {
        let mut store = open_store(path);
        store
            .execute(BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 800,
            })
            .unwrap();
        // Intentionally transfers to a nonexistent account to produce a cached failure.
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "ghost".into(),
                amount: 100,
                client_tx_id: "tx-det".into(),
            })
            .unwrap();
    }

    let store1 = open_store(path);
    let store2 = open_store(path);

    assert_eq!(store1.get_balance("alice"), store2.get_balance("alice"));
    assert_eq!(
        store1.get_transfer_status("tx-det"),
        store2.get_transfer_status("tx-det")
    );
}

/// Opening a store with an empty WAL produces empty state.
#[test]
fn open_empty_store_has_no_state() {
    let tmp = NamedTempFile::new().unwrap();
    let store = open_store(tmp.path().to_str().unwrap());
    assert_eq!(store.get_balance("nobody"), None);
}

/// A duplicate CreateAccount command is applied idempotently — the second
/// call is logged and replayed but the original balance is preserved.
#[test]
fn duplicate_create_account_preserves_balance() {
    let tmp = NamedTempFile::new().unwrap();
    let mut store = open_store(tmp.path().to_str().unwrap());

    store
        .execute(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 100,
        })
        .unwrap();
    store
        .execute(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 999,
        })
        .unwrap();

    assert_eq!(store.get_balance("alice"), Some(100));
}

/// A transfer retried with the same client_tx_id is only applied once, even
/// across a restart.
#[test]
fn idempotent_transfer_persisted_across_restart() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    {
        let mut store = open_store(path);
        store
            .execute(BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 500,
            })
            .unwrap();
        store
            .execute(BankCommand::CreateAccount {
                account_id: "bob".into(),
                initial_balance: 0,
            })
            .unwrap();
        // First attempt.
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 100,
                client_tx_id: "tx-idem".into(),
            })
            .unwrap();
        // Retry in the same session.
        store
            .execute(BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 100,
                client_tx_id: "tx-idem".into(),
            })
            .unwrap();
    }

    // Reopen and retry again — should still only have been applied once.
    let mut store = open_store(path);
    store
        .execute(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 100,
            client_tx_id: "tx-idem".into(),
        })
        .unwrap();

    assert_eq!(store.get_balance("alice"), Some(400));
    assert_eq!(store.get_balance("bob"), Some(100));
}
