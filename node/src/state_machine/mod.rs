pub(crate) mod command;

pub use command::{BankCommand, DecodeError};

use std::collections::HashMap;

use crate::wal::entry::LogEntry;

/// Result of a `CreateAccount` command.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum CreateAccountResult {
    Ok,
    AlreadyExists,
}

/// Result of a `Transfer` command.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TransferResult {
    Ok,
    InsufficientFunds,
    InvalidAccount,
}

/// Typed wrapper around the two possible `apply` outcomes.
#[derive(Debug, PartialEq)]
pub enum ApplyResult {
    CreateAccount(CreateAccountResult),
    Transfer(TransferResult),
}

/// Pure in-memory projection of all committed WAL entries.
#[derive(Debug, Default)]
pub struct StateMachine {
    last_applied_index: u64,
    accounts: HashMap<String, i64>,
    dedupe: HashMap<String, TransferResult>,
}

impl StateMachine {
    fn new() -> Self {
        Self::default()
    }

    /// Returns the index of the last-applied WAL entry.
    #[allow(dead_code)]
    pub fn last_applied_index(&self) -> u64 {
        self.last_applied_index
    }

    /// Reconstruct state by replaying all WAL entries from scratch.
    pub fn restore(entries: Vec<LogEntry>) -> Result<Self, DecodeError> {
        let mut sm = Self::new();
        for entry in entries {
            let cmd = BankCommand::decode_from_bytes(&entry.command)?;
            sm.apply(entry.index, cmd);
        }
        Ok(sm)
    }

    /// Apply a single command, mutating state. Returns the outcome.
    /// This is the only path through which state ever changes.
    pub fn apply(&mut self, index: u64, cmd: BankCommand) -> ApplyResult {
        // TODO: replace panic with error handling and let caller decide how to proceed (e.g. crash, skip, etc.)
        assert_eq!(
            index,
            self.last_applied_index + 1,
            "WAL entries must be applied in order without gaps"
        );

        let apply_result = match cmd {
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
        };

        self.last_applied_index = index;

        apply_result
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
    ) -> TransferResult {
        // Idempotency: return the cached outcome if already seen.
        if let Some(&result) = self.dedupe.get(&client_tx_id) {
            return result;
        }

        let result = if !self.accounts.contains_key(&from) || !self.accounts.contains_key(&to) {
            TransferResult::InvalidAccount
        } else if self.accounts[&from] < amount {
            TransferResult::InsufficientFunds
        } else {
            *self.accounts.get_mut(&from).unwrap() -= amount;
            *self.accounts.get_mut(&to).unwrap() += amount;
            TransferResult::Ok
        };

        self.dedupe.insert(client_tx_id, result);
        result
    }

    // ---- Read-only accessors ------------------------------------------------

    /// Returns the current balance, or `None` if the account does not exist.
    pub fn get_balance(&self, account_id: &str) -> Option<i64> {
        self.accounts.get(account_id).copied()
    }

    /// Returns the cached transfer outcome, or `None` if the `client_tx_id`
    /// has never been submitted.
    pub fn get_transfer_status(&self, client_tx_id: &str) -> Option<TransferResult> {
        self.dedupe.get(client_tx_id).copied()
    }
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
        let result = sm.apply(
            1,
            BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 500,
            },
        );
        assert_eq!(result, ApplyResult::CreateAccount(CreateAccountResult::Ok));
        assert_eq!(sm.get_balance("alice"), Some(500));
    }

    #[test]
    fn create_account_duplicate() {
        let mut sm = StateMachine::new();
        sm.apply(
            1,
            BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 500,
            },
        );
        let result = sm.apply(
            2,
            BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 999,
            },
        );
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
        sm.apply(
            1,
            BankCommand::CreateAccount {
                account_id: "alice".into(),
                initial_balance: 1000,
            },
        );
        sm.apply(
            2,
            BankCommand::CreateAccount {
                account_id: "bob".into(),
                initial_balance: 0,
            },
        );
        sm
    }

    #[test]
    fn transfer_ok() {
        let mut sm = seeded_sm();
        let result = sm.apply(
            3,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 300,
                client_tx_id: "tx-1".into(),
            },
        );
        assert_eq!(result, ApplyResult::Transfer(TransferResult::Ok));
        assert_eq!(sm.get_balance("alice"), Some(700));
        assert_eq!(sm.get_balance("bob"), Some(300));
    }

    #[test]
    fn transfer_insufficient_funds() {
        let mut sm = seeded_sm();
        let result = sm.apply(
            3,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 9999,
                client_tx_id: "tx-2".into(),
            },
        );
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferResult::InsufficientFunds)
        );
        // Balances unchanged
        assert_eq!(sm.get_balance("alice"), Some(1000));
        assert_eq!(sm.get_balance("bob"), Some(0));
    }

    #[test]
    fn transfer_invalid_account() {
        let mut sm = seeded_sm();
        let result = sm.apply(
            3,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "nobody".into(),
                amount: 100,
                client_tx_id: "tx-3".into(),
            },
        );
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferResult::InvalidAccount)
        );
    }

    #[test]
    fn transfer_idempotent_success() {
        let mut sm = seeded_sm();
        sm.apply(
            3,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 100,
                client_tx_id: "tx-4".into(),
            },
        );
        // Retry with the same client_tx_id
        let result = sm.apply(
            4,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 100,
                client_tx_id: "tx-4".into(),
            },
        );
        assert_eq!(result, ApplyResult::Transfer(TransferResult::Ok));
        // Applied only once
        assert_eq!(sm.get_balance("alice"), Some(900));
        assert_eq!(sm.get_balance("bob"), Some(100));
    }

    #[test]
    fn transfer_idempotent_failure() {
        let mut sm = seeded_sm();
        sm.apply(
            3,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 9999,
                client_tx_id: "tx-5".into(),
            },
        );
        let result = sm.apply(
            4,
            BankCommand::Transfer {
                from: "alice".into(),
                to: "bob".into(),
                amount: 9999,
                client_tx_id: "tx-5".into(),
            },
        );
        assert_eq!(
            result,
            ApplyResult::Transfer(TransferResult::InsufficientFunds)
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
        let cmds = [
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
            Some(TransferResult::Ok)
        );
    }
}
