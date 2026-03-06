use crate::state_machine::{ApplyResult, BankCommand, StateMachine, TransferResult};
use crate::wal::wal::Wal;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("WAL error: {0}")]
    WalError(#[from] std::io::Error),
    #[error("state machine error: {0}")]
    StateMachineError(#[from] crate::state_machine::command::DecodeError),
}

#[derive(Debug)]
pub struct Store {
    wal: Wal,
    state_machine: StateMachine,
}

impl Store {
    pub fn open(wal_path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let wal = Wal::new(wal_path)?;

        // Replay WAL entries to restore state machine.
        let entries = wal.replay()?;
        let state_machine = StateMachine::restore(entries)?;

        Ok(Self { wal, state_machine })
    }

    pub fn execute(&mut self, cmd: BankCommand) -> Result<ApplyResult, StoreError> {
        // Encode command and determine next log index.
        let payload = cmd.encode_to_bytes();
        let index = self.wal.last_index() + 1;

        // Append command to WAL.
        self.wal.append(crate::wal::entry::LogEntry {
            index,
            term: 0, // Term is not used in this simplified example.
            command: payload,
        })?;

        // Apply command to state machine.
        let result = self.state_machine.apply(index, cmd);

        Ok(result)
    }

    pub fn get_balance(&self, account_id: &str) -> Option<i64> {
        self.state_machine.get_balance(account_id)
    }

    pub fn get_transfer_status(&self, client_tx_id: &str) -> Option<TransferResult> {
        self.state_machine.get_transfer_status(client_tx_id)
    }
}
