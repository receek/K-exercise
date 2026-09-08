use std::collections::HashMap;
use std::fmt;

use rust_decimal::Decimal;
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::transaction::Transaction;

#[derive(Debug)]
pub enum ClientError {
    AccountLocked,
    InsufficientFunds,
    InvalidTransaction,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::AccountLocked => write!(f, "account is locked"),
            ClientError::InsufficientFunds => write!(f, "insufficient available funds"),
            ClientError::InvalidTransaction => write!(f, "invalid transaction"),
        }
    }
}

impl std::error::Error for ClientError {}

pub struct Client {
    pub client_id: u16,
    pub available: Decimal,
    pub held: Decimal,
    pub locked: bool,
    // holds `Deposit` and `Withdraw` transaction history
    pub transactions: HashMap<u32, Transaction>,
    // holds disputed `Deposit` and `Withdraw` transactions
    pub disputes: HashMap<u32, Transaction>,
}

impl Serialize for Client {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Client", 5)?;
        state.serialize_field("client", &self.client_id)?;
        state.serialize_field("available", &self.available)?;
        state.serialize_field("held", &self.held)?;
        state.serialize_field("total", &(self.available + self.held))?;
        state.serialize_field("locked", &self.locked)?;
        state.end()
    }
}

impl Client {
    pub fn new(client_id: u16) -> Self {
        Client {
            client_id,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            locked: false,
            transactions: HashMap::new(),
            disputes: HashMap::new(),
        }
    }

    fn add_available(&mut self, amount: Decimal) {
        self.available = (self.available + amount).round_dp(4);
    }

    fn subtract_available(&mut self, amount: Decimal) {
        self.available = (self.available - amount).round_dp(4);
    }

    fn add_held(&mut self, amount: Decimal) {
        self.held = (self.held + amount).round_dp(4);
    }

    fn subtract_held(&mut self, amount: Decimal) {
        self.held = (self.held - amount).round_dp(4);
    }

    pub fn process_transaction(&mut self, transaction: Transaction) -> Result<(), ClientError> {
        match transaction {
            Transaction::Deposit { .. } | Transaction::Withdraw { .. } if self.locked => {
                Err(ClientError::AccountLocked)
            }
            Transaction::Deposit { tx, amount, .. } => {
                self.add_available(amount);
                self.transactions.insert(tx, transaction);
                Ok(())
            }
            Transaction::Withdraw { tx, amount, .. } => {
                if self.available < amount {
                    return Err(ClientError::InsufficientFunds);
                }
                self.subtract_available(amount);
                self.transactions.insert(tx, transaction);
                Ok(())
            }

            Transaction::Dispute { tx, client } => match self.transactions.remove(&tx) {
                Some(Transaction::Deposit { amount, .. }) => {
                    /*
                    Dispute to `Deposit` transaction, move `amount` from `available` to `held`.
                    */
                    self.add_held(amount);
                    self.subtract_held(amount);

                    self.disputes
                        .insert(tx, Transaction::Deposit { amount, tx, client });
                    Ok(())
                }
                Some(Transaction::Withdraw { amount, .. }) => {
                    /*
                    Dispute to `Withdraw` transaction, add `amount` to `held`.
                    */
                    self.add_held(amount);

                    self.disputes
                        .insert(tx, Transaction::Withdraw { client, tx, amount });
                    Ok(())
                }
                _ => Err(ClientError::InvalidTransaction),
            },
            Transaction::Resolve { tx, .. } => match self.disputes.remove(&tx) {
                Some(Transaction::Deposit { amount, .. }) => {
                    /*
                    Reject dispute of `Deposit` transaction, move `amount` from `held` to `available` .
                    */
                    self.subtract_held(amount);
                    self.add_available(amount);

                    Ok(())
                }
                Some(Transaction::Withdraw { amount, .. }) => {
                    /*
                    Reject dispute of `Withdraw` transaction, subtract `amount` from `held`.
                    */
                    self.add_held(amount);

                    Ok(())
                }
                _ => Err(ClientError::InvalidTransaction),
            },
            Transaction::Chargeback { tx, .. } => match self.disputes.remove(&tx) {
                Some(Transaction::Deposit { amount, .. }) => {
                    /*
                    Reverse `Deposit` transaction, subtract `amount` from `held`.
                    */
                    self.subtract_held(amount);

                    Ok(())
                }
                Some(Transaction::Withdraw { amount, .. }) => {
                    /*
                    Reverse of `Withdraw` transaction, move `amount` from `held` to `available`.
                    */
                    self.subtract_held(amount);
                    self.add_available(amount);

                    Ok(())
                }
                _ => Err(ClientError::InvalidTransaction),
            },
        }
    }
}
