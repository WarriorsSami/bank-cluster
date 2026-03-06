//! In-process gRPC integration tests.
//!
//! Each test spins up a real tonic server bound to a random port, connects a
//! generated client to it, and exercises the full request → service → store →
//! WAL → state machine → response path.  A fresh temporary WAL file is used
//! per test so tests are fully isolated.
use crate::service::service::BankGrpcService;
use crate::store::store::Store;
use bank_api::bank::bank_service_client::BankServiceClient;
use bank_api::bank::bank_service_server::BankServiceServer;
use bank_api::bank::{
    AccountId, ClientTxId, CreateAccountRequest, GetBalanceRequest, GetTransferStatusRequest,
    TransferRequest, TransferStatus,
};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tokio::sync::Mutex;
use tonic::Code;
use tonic::codegen::tokio_stream;
use tonic::transport::{Channel, Server};

// ---- Helpers ----------------------------------------------------------------

/// Spawn an in-process server on a random OS-assigned port and return a
/// connected client.  The server is shut down when the returned
/// `ServerHandle` is dropped.
async fn spawn_server(wal_path: &str) -> (BankServiceClient<Channel>, ServerHandle) {
    let store = Arc::new(Mutex::new(Store::open(wal_path).unwrap()));
    let service = BankGrpcService::new(store);

    // Port 0 → OS picks a free port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        Server::builder()
            .add_service(BankServiceServer::new(service))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = rx.await;
            })
            .await
            .unwrap();
    });

    let endpoint = format!("http://{addr}");
    // Retry briefly while the server starts.
    let client = tokio_retry::Retry::spawn(
        tokio_retry::strategy::FixedInterval::from_millis(10).take(20),
        || BankServiceClient::connect(endpoint.clone()),
    )
    .await
    .unwrap();

    (client, ServerHandle(tx))
}

/// Dropping this shuts the server down.
#[allow(dead_code)]
struct ServerHandle(tokio::sync::oneshot::Sender<()>);

fn account(id: &str) -> AccountId {
    AccountId { id: id.into() }
}

fn tx_id(id: &str) -> ClientTxId {
    ClientTxId { id: id.into() }
}

// ---- Tests ------------------------------------------------------------------

#[tokio::test]
async fn create_account_success() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    let resp = client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 1000,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(resp.success);
}

#[tokio::test]
async fn create_account_duplicate_returns_failure() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 500,
        })
        .await
        .unwrap();

    let resp = client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 999,
        })
        .await
        .unwrap()
        .into_inner();

    assert!(!resp.success);
    // Balance unchanged — original account wins.
    let balance = client
        .get_balance(GetBalanceRequest {
            account: Some(account("alice")),
        })
        .await
        .unwrap()
        .into_inner()
        .balance;
    assert_eq!(balance, Some(500));
}

#[tokio::test]
async fn get_balance_unknown_account_returns_not_found() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    let err = client
        .get_balance(GetBalanceRequest {
            account: Some(account("ghost")),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), Code::NotFound);
}

#[tokio::test]
async fn transfer_ok_updates_balances() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 1000,
        })
        .await
        .unwrap();
    client
        .create_account(CreateAccountRequest {
            account: Some(account("bob")),
            initial_balance: 0,
        })
        .await
        .unwrap();

    let resp = client
        .transfer(TransferRequest {
            from: Some(account("alice")),
            to: Some(account("bob")),
            amount: 400,
            client_tx_id: Some(tx_id("tx-1")),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.status(), TransferStatus::CommittedOk);

    let alice_balance = client
        .get_balance(GetBalanceRequest {
            account: Some(account("alice")),
        })
        .await
        .unwrap()
        .into_inner()
        .balance;
    let bob_balance = client
        .get_balance(GetBalanceRequest {
            account: Some(account("bob")),
        })
        .await
        .unwrap()
        .into_inner()
        .balance;

    assert_eq!(alice_balance, Some(600));
    assert_eq!(bob_balance, Some(400));
}

#[tokio::test]
async fn transfer_insufficient_funds() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 100,
        })
        .await
        .unwrap();
    client
        .create_account(CreateAccountRequest {
            account: Some(account("bob")),
            initial_balance: 0,
        })
        .await
        .unwrap();

    let resp = client
        .transfer(TransferRequest {
            from: Some(account("alice")),
            to: Some(account("bob")),
            amount: 9999,
            client_tx_id: Some(tx_id("tx-2")),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.status(), TransferStatus::CommittedInsufficientFunds);

    // Balances unchanged.
    let alice_balance = client
        .get_balance(GetBalanceRequest {
            account: Some(account("alice")),
        })
        .await
        .unwrap()
        .into_inner()
        .balance;
    assert_eq!(alice_balance, Some(100));
}

#[tokio::test]
async fn transfer_invalid_account() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 500,
        })
        .await
        .unwrap();

    let resp = client
        .transfer(TransferRequest {
            from: Some(account("alice")),
            to: Some(account("nobody")),
            amount: 100,
            client_tx_id: Some(tx_id("tx-3")),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.status(), TransferStatus::CommittedInvalidAccount);
}

#[tokio::test]
async fn transfer_idempotent_across_retries() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 500,
        })
        .await
        .unwrap();
    client
        .create_account(CreateAccountRequest {
            account: Some(account("bob")),
            initial_balance: 0,
        })
        .await
        .unwrap();

    // First attempt.
    client
        .transfer(TransferRequest {
            from: Some(account("alice")),
            to: Some(account("bob")),
            amount: 100,
            client_tx_id: Some(tx_id("tx-idem")),
        })
        .await
        .unwrap();

    // Retry — same client_tx_id.
    let resp = client
        .transfer(TransferRequest {
            from: Some(account("alice")),
            to: Some(account("bob")),
            amount: 100,
            client_tx_id: Some(tx_id("tx-idem")),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(resp.status(), TransferStatus::CommittedOk);

    // Transfer was only applied once.
    let alice_balance = client
        .get_balance(GetBalanceRequest {
            account: Some(account("alice")),
        })
        .await
        .unwrap()
        .into_inner()
        .balance;
    assert_eq!(alice_balance, Some(400));
}

#[tokio::test]
async fn get_transfer_status_returns_cached_result() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    client
        .create_account(CreateAccountRequest {
            account: Some(account("alice")),
            initial_balance: 500,
        })
        .await
        .unwrap();
    client
        .create_account(CreateAccountRequest {
            account: Some(account("bob")),
            initial_balance: 0,
        })
        .await
        .unwrap();
    client
        .transfer(TransferRequest {
            from: Some(account("alice")),
            to: Some(account("bob")),
            amount: 200,
            client_tx_id: Some(tx_id("tx-status")),
        })
        .await
        .unwrap();

    let status = client
        .get_transfer_status(GetTransferStatusRequest {
            client_tx_id: Some(tx_id("tx-status")),
        })
        .await
        .unwrap()
        .into_inner()
        .status();

    assert_eq!(status, TransferStatus::CommittedOk);
}

#[tokio::test]
async fn get_transfer_status_unknown_tx_returns_not_found() {
    let tmp = NamedTempFile::new().unwrap();
    let (mut client, _srv) = spawn_server(tmp.path().to_str().unwrap()).await;

    let err = client
        .get_transfer_status(GetTransferStatusRequest {
            client_tx_id: Some(tx_id("never-submitted")),
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), Code::NotFound);
}
