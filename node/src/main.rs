// TODO: Amend the modules layout.
mod service;
mod state_machine;
mod store;
mod wal;

use crate::service::service::BankGrpcService;
use crate::store::store::Store;
use bank_api::bank::bank_service_server::BankServiceServer;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wal_path = "node.wal";
    let store = Arc::new(Mutex::new(Store::open(wal_path)?));
    let service = BankGrpcService::new(store);

    let addr = "[::1]:50051".parse()?;
    println!("BankService listening on {addr}");

    Server::builder()
        .add_service(BankServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
