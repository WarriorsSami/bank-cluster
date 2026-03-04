use bank_api::bank::{
    bank_command::Command, AccountId, BankCommand as ProtoBankCommand, ClientTxId,
    CreateAccountRequest, TransferRequest,
};
use bytes::Bytes;
use prost::Message;

/// Domain-level command enum used by the state machine.
/// Fields are unpacked scalars — no proto types leak past this boundary.
#[derive(Debug, Clone)]
pub enum BankCommand {
    CreateAccount {
        account_id: String,
        initial_balance: i64,
    },
    Transfer {
        from: String,
        to: String,
        amount: i64,
        client_tx_id: String,
    },
}

impl BankCommand {
    /// Encode this command as proto bytes suitable for storage in `LogEntry::command`.
    pub fn encode_to_bytes(&self) -> Bytes {
        let proto = match self {
            BankCommand::CreateAccount {
                account_id,
                initial_balance,
            } => ProtoBankCommand {
                command: Some(Command::CreateAccount(CreateAccountRequest {
                    account: Some(AccountId {
                        id: account_id.clone(),
                    }),
                    initial_balance: *initial_balance,
                })),
            },
            BankCommand::Transfer {
                from,
                to,
                amount,
                client_tx_id,
            } => ProtoBankCommand {
                command: Some(Command::Transfer(TransferRequest {
                    from: Some(AccountId { id: from.clone() }),
                    to: Some(AccountId { id: to.clone() }),
                    amount: *amount,
                    client_tx_id: Some(ClientTxId {
                        id: client_tx_id.clone(),
                    }),
                })),
            },
        };

        Bytes::from(proto.encode_to_vec())
    }

    /// Decode a `BankCommand` from proto bytes stored in `LogEntry::command`.
    pub fn decode_from_bytes(bytes: &Bytes) -> Result<Self, DecodeError> {
        let proto = ProtoBankCommand::decode(bytes.as_ref()).map_err(DecodeError::Proto)?;

        match proto.command {
            Some(Command::CreateAccount(req)) => {
                let account_id = req
                    .account
                    .ok_or(DecodeError::MissingField("account"))?
                    .id;
                Ok(BankCommand::CreateAccount {
                    account_id,
                    initial_balance: req.initial_balance,
                })
            }
            Some(Command::Transfer(req)) => {
                let from = req.from.ok_or(DecodeError::MissingField("from"))?.id;
                let to = req.to.ok_or(DecodeError::MissingField("to"))?.id;
                let client_tx_id = req
                    .client_tx_id
                    .ok_or(DecodeError::MissingField("client_tx_id"))?
                    .id;
                Ok(BankCommand::Transfer {
                    from,
                    to,
                    amount: req.amount,
                    client_tx_id,
                })
            }
            None => Err(DecodeError::EmptyCommand),
        }
    }
}

#[derive(Debug)]
pub enum DecodeError {
    Proto(prost::DecodeError),
    MissingField(&'static str),
    EmptyCommand,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Proto(e) => write!(f, "proto decode error: {e}"),
            DecodeError::MissingField(field) => write!(f, "missing required field: {field}"),
            DecodeError::EmptyCommand => write!(f, "BankCommand oneof is empty"),
        }
    }
}

impl std::error::Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_create_account() {
        let cmd = BankCommand::CreateAccount {
            account_id: "alice".to_string(),
            initial_balance: 1000,
        };
        let bytes = cmd.encode_to_bytes();
        let decoded = BankCommand::decode_from_bytes(&bytes).unwrap();
        match decoded {
            BankCommand::CreateAccount {
                account_id,
                initial_balance,
            } => {
                assert_eq!(account_id, "alice");
                assert_eq!(initial_balance, 1000);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn roundtrip_transfer() {
        let cmd = BankCommand::Transfer {
            from: "alice".to_string(),
            to: "bob".to_string(),
            amount: 250,
            client_tx_id: "tx-1".to_string(),
        };
        let bytes = cmd.encode_to_bytes();
        let decoded = BankCommand::decode_from_bytes(&bytes).unwrap();
        match decoded {
            BankCommand::Transfer {
                from,
                to,
                amount,
                client_tx_id,
            } => {
                assert_eq!(from, "alice");
                assert_eq!(to, "bob");
                assert_eq!(amount, 250);
                assert_eq!(client_tx_id, "tx-1");
            }
            _ => panic!("unexpected variant"),
        }
    }
}