use crate::state_machine::{ApplyResult, BankCommand, CreateAccountResult, TransferResult};
use crate::store::store::Store;
use bank_api::bank::bank_service_server::BankService;
use bank_api::bank::{
    CreateAccountRequest, CreateAccountResponse, GetBalanceRequest, GetBalanceResponse,
    GetTransferStatusRequest, GetTransferStatusResponse, TransferRequest, TransferResponse,
    TransferStatus,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

#[derive(Debug, Clone)]
pub struct BankGrpcService {
    store: Arc<Mutex<Store>>,
}

impl BankGrpcService {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self { store }
    }
}

impl From<TransferResult> for TransferStatus {
    fn from(result: TransferResult) -> Self {
        match result {
            TransferResult::Ok => TransferStatus::CommittedOk,
            TransferResult::InsufficientFunds => TransferStatus::CommittedInsufficientFunds,
            TransferResult::InvalidAccount => TransferStatus::CommittedInvalidAccount,
        }
    }
}

#[tonic::async_trait]
impl BankService for BankGrpcService {
    async fn create_account(
        &self,
        request: Request<CreateAccountRequest>,
    ) -> Result<Response<CreateAccountResponse>, Status> {
        let req = request.into_inner();
        let cmd = BankCommand::CreateAccount {
            account_id: req.account.map(|a| a.id).unwrap_or_default(),
            initial_balance: req.initial_balance,
        };

        let mut store = self.store.lock().await;
        match store
            .execute(cmd)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            ApplyResult::CreateAccount(CreateAccountResult::Ok) => {
                Ok(Response::new(CreateAccountResponse {
                    success: true,
                    message: "Account created".into(),
                }))
            }
            ApplyResult::CreateAccount(CreateAccountResult::AlreadyExists) => {
                Ok(Response::new(CreateAccountResponse {
                    success: false,
                    message: "Account already exists".into(),
                }))
            }
            _ => Err(Status::internal("unexpected result")),
        }
    }

    async fn get_balance(
        &self,
        request: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
        let req = request.into_inner();
        let account_id = req.account.map(|a| a.id).unwrap_or_default();

        let store = self.store.lock().await;
        match store.get_balance(&account_id) {
            Some(balance) => Ok(Response::new(GetBalanceResponse { balance: Some(balance) })),
            None => Err(Status::not_found(format!(
                "account '{account_id}' not found"
            ))),
        }
    }

    async fn transfer(
        &self,
        request: Request<TransferRequest>,
    ) -> Result<Response<TransferResponse>, Status> {
        let req = request.into_inner();
        let cmd = BankCommand::Transfer {
            from: req.from.map(|a| a.id).unwrap_or_default(),
            to: req.to.map(|a| a.id).unwrap_or_default(),
            amount: req.amount,
            client_tx_id: req.client_tx_id.map(|t| t.id).unwrap_or_default(),
        };

        let mut store = self.store.lock().await;
        match store
            .execute(cmd)
            .map_err(|e| Status::internal(e.to_string()))?
        {
            ApplyResult::Transfer(result) => {
                let message = match result {
                    TransferResult::Ok => "Transfer committed".into(),
                    TransferResult::InsufficientFunds => "Insufficient funds".into(),
                    TransferResult::InvalidAccount => "Invalid account".into(),
                };

                Ok(Response::new(TransferResponse {
                    status: TransferStatus::from(result).into(),
                    message,
                }))
            }
            _ => Err(Status::internal("unexpected result")),
        }
    }

    async fn get_transfer_status(
        &self,
        request: Request<GetTransferStatusRequest>,
    ) -> Result<Response<GetTransferStatusResponse>, Status> {
        let req = request.into_inner();
        let client_tx_id = req.client_tx_id.map(|t| t.id).unwrap_or_default();

        let store = self.store.lock().await;
        match store.get_transfer_status(&client_tx_id) {
            Some(result) => Ok(Response::new(GetTransferStatusResponse {
                status: TransferStatus::from(result).into(),
                message: String::new(),
            })),
            None => Err(Status::not_found(format!(
                "transaction '{client_tx_id}' not found"
            ))),
        }
    }
}
