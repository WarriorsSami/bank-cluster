mod wal;
mod state_machine;

use std::sync::{Arc, Mutex};
use state_machine::StateMachine;
use crate::wal::wal::Wal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wal_path = "node.wal";

    let wal = Wal::new(wal_path)?;
    let entries = wal.replay()?;
    let sm = StateMachine::restore(entries)?;

    let _sm = Arc::new(Mutex::new(sm));

    Ok(())
}
