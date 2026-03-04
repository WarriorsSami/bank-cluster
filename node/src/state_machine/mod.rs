mod command;
#[cfg(test)]
mod integration_tests;

pub use command::{BankCommand, DecodeError};

use bank_api::bank::TransferStatus;
use std::collections::HashMap;

use crate::wal::entry::LogEntry;

/// Result of a `CreateAccount` command.
#[derive(Debug, PartialEq)]
pub enum CreateAccountResult {
    Ok,
    AlreadyExists,
}

/// Pure in-memory projection of all committed WAL entries.
#[derive(Debug, Default)]
pub struct StateMachine {
    accounts: HashMap<String, i64>,
    dedupe: HashMap<String, TransferStatus>,
}

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconstruct state by replaying all WAL entries from scratch.
    pub fn restore(entries: Vec<LogEntry>) -> Result<Self, DecodeError> {
        let mut sm = Self::new();
        for entry in entries {
            let cmd = BankCommand::decode_from_bytes(&entry.command)?;
            sm.apply(cmd);
        }
        Ok(sm)
    }

    /// Apply a single command, mutating state. Returns the outcome.
    /// This is the only path through which state ever changes.
    pub fn apply(&mut self, cmd: BankCommand) -> ApplyResult {
        match cmd {
            BankCommand::CreateAccount {
                account_id,
                initial_balance,
            } => ApplyResult::CreateAccount(self.apply_create_account(account_id, initial_balance)),

            BankCommand::Transfer {
                from,
                to,
                amount,
                client_tx_id,
            } => ApplyResult::Transfer(self.apply_transfer(from, to, amount, client_tx_id)),
        }
    }

    fn apply_create_account(
        &mut self,
        account_id: String,
        initial_balance: i64,
    ) -> CreateAccountResult {
        if self.accounts.contains_key(&account_id) {
            return CreateAccountResult::AlreadyExists;
        }
        self.accounts.insert(account_id, initial_balance);
        CreateAccountResult::Ok
    }

    fn apply_transfer(
        &mut self,
        from: String,
        to: String,
        amount: i64,
        client_tx_id: String,
    ) -> TransferStatus {
        // Idempotency: return the cached outcome if already seen.
        if let Some(&status) = self.dedupe.get(&client_tx_id) {
            return status;
        }

        let status = if !self.accounts.contains_key(&from) || !self.accounts.contains_key(&to) {
            TransferStatus::CommittedInvalidAccount
        } else if self.accounts[&from] < amount {
            TransferStatus::CommittedInsufficientFunds
        } else {
            *self.accounts.get_mut(&from).unwrap() -= amount;
            *self.accounts.get_mut(&to).unwrap() += amount;
            TransferStatus::CommittedOk
        };
        
        self.dedupe.insert(client_tx_id, status);
        status
    }

    // ---- Read-only accessors ------------------------------------------------

    /// Returns the current balance, or `None` if the account does not exist.
    pub fn get_balance(&self, account_id: &str) -> Option<i64> {
        self.accounts.get(account_id).copied()
    }

    /// Returns the cached transfer outcome, or `None` if the `client_tx_id`
    /// has never been submitted.
    pub fn get_transfer_status(&self, client_tx_id: &str) -> Option<TransferStatus> {
        self.dedupe.get(client_tx_id).copied()
    }
}

/// Typed wrapper around the two possible `apply` outcomes.
#[derive(Debug, PartialEq)]
pub enum ApplyResult {
    CreateAccount(CreateAccountResult),
    Transfer(TransferStatus),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(index: u64, cmd: &BankCommand) -> LogEntry {
        LogEntry {
            index,
            term: 0,
            command: cmd.encode_to_bytes(),
        }
    }

    // ---- CreateAccount ------------------------------------------------------

    #[test]
    fn create_account_ok() {
        let mut sm = StateMachine::new();
        let result = sm.apply(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 500,
        });
        assert_eq!(result, ApplyResult::CreateAccount(CreateAccountResult::Ok));
        assert_eq!(sm.get_balance("alice"), Some(500));
    }

    #[test]
    fn create_account_duplicate() {
        let mut sm = StateMachine::new();
        sm.apply(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 500,
        });
        let result = sm.apply(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 999,
        });
        assert_eq!(
            result,
            ApplyResult::CreateAccount(CreateAccountResult::AlreadyExists)
        );
        // Balance unchanged
        assert_eq!(sm.get_balance("alice"), Some(500));
    }

    // ---- Transfer -----------------------------------------------------------

    fn seeded_sm() -> StateMachine {
        let mut sm = StateMachine::new();
        sm.apply(BankCommand::CreateAccount {
            account_id: "alice".into(),
            initial_balance: 1000,
        });
        sm.apply(BankCommand::CreateAccount {
            account_id: "bob".into(),
            initial_balance: 0,
        });
        sm
    }

    #[test]
    fn transfer_ok() {
        let mut sm = seeded_sm();
        let result = sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 300,
            client_tx_id: "tx-1".into(),
        });
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferStatus::CommittedOk)
        );
        assert_eq!(sm.get_balance("alice"), Some(700));
        assert_eq!(sm.get_balance("bob"), Some(300));
    }

    #[test]
    fn transfer_insufficient_funds() {
        let mut sm = seeded_sm();
        let result = sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 9999,
            client_tx_id: "tx-2".into(),
        });
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferStatus::CommittedInsufficientFunds)
        );
        // Balances unchanged
        assert_eq!(sm.get_balance("alice"), Some(1000));
        assert_eq!(sm.get_balance("bob"), Some(0));
    }

    #[test]
    fn transfer_invalid_account() {
        let mut sm = seeded_sm();
        let result = sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "nobody".into(),
            amount: 100,
            client_tx_id: "tx-3".into(),
        });
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferStatus::CommittedInvalidAccount)
        );
    }

    #[test]
    fn transfer_idempotent_success() {
        let mut sm = seeded_sm();
        sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 100,
            client_tx_id: "tx-4".into(),
        });
        // Retry with the same client_tx_id
        let result = sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 100,
            client_tx_id: "tx-4".into(),
        });
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferStatus::CommittedOk)
        );
        // Applied only once
        assert_eq!(sm.get_balance("alice"), Some(900));
        assert_eq!(sm.get_balance("bob"), Some(100));
    }

    #[test]
    fn transfer_idempotent_failure() {
        let mut sm = seeded_sm();
        sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 9999,
            client_tx_id: "tx-5".into(),
        });
        let result = sm.apply(BankCommand::Transfer {
            from: "alice".into(),
            to: "bob".into(),
            amount: 9999,
            client_tx_id: "tx-5".into(),
        });
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferStatus::CommittedInsufficientFunds)
        );
    }

    // ---- Reads --------------------------------------------------------------

    #[test]
    fn get_balance_missing_account() {
        let sm = StateMachine::new();
        assert_eq!(sm.get_balance("ghost"), None);
    }

    #[test]
    fn get_transfer_status_unknown_tx() {
        let sm = StateMachine::new();
        assert_eq!(sm.get_transfer_status("never-submitted"), None);
    }

    // ---- Restore from WAL ---------------------------------------------------

    #[test]
    fn restore_from_wal_entries() {
        let cmds = vec![
            BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 500,
            },
            BankCommand::CreateAccount {
                account_id: "bob".into(),
                initial_balance: 0,
            },
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 200,
                client_tx_id: "tx-restore".into(),
            },
        ];

        let entries: Vec<LogEntry> = cmds
            .iter()
            .enumerate()
            .map(|(i, cmd)| make_entry((i + 1) as u64, cmd))
            .collect();

        let sm = StateMachine::restore(entries).unwrap();

        assert_eq!(sm.get_balance("alice"), Some(300));
        assert_eq!(sm.get_balance("bob"), Some(200));
        assert_eq!(
            sm.get_transfer_status("tx-restore"),
            Some(TransferStatus::CommittedOk)
        );
    }
}
