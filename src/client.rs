use std::collections::{HashMap, HashSet};
use std::fmt;

use rust_decimal::Decimal;
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::transaction::Transaction;

// Subset of `Transaction` holding only the funds-moving variants, used to
// remember transaction history for `Dispute`/`Resolve`/`Chargeback` lookups.
// Internal to `Client` — never exposed outside this module.
#[derive(Debug)]
enum FundsTransaction {
    Deposit { amount: Decimal },
    Withdrawal { amount: Decimal },
}

#[derive(Debug)]
pub enum ClientError {
    AccountLocked,
    InsufficientFunds,
    InvalidTransaction,
    TransactionReferenceConflict,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::AccountLocked => write!(f, "account is locked"),
            ClientError::InsufficientFunds => write!(f, "insufficient available funds"),
            ClientError::InvalidTransaction => write!(f, "invalid transaction"),
            ClientError::TransactionReferenceConflict => {
                write!(f, "transaction identifier conflict")
            }
        }
    }
}

impl std::error::Error for ClientError {}

pub struct Client {
    pub client_id: u16,
    pub available: Decimal,
    pub held: Decimal,
    pub locked: bool,
    // holds `Deposit` and `Withdrawal` transaction history
    transactions: HashMap<u32, FundsTransaction>,
    // holds disputed `Deposit` and `Withdrawal` transactions
    disputes: HashMap<u32, FundsTransaction>,
    // holds resolved or chargebacked txs to detect tx conflict
    handled_disputes: HashSet<u32>,
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
            handled_disputes: HashSet::new(),
        }
    }

    fn add_available(&mut self, amount: &Decimal) {
        self.available = (self.available + amount).round_dp(4);
    }

    fn subtract_available(&mut self, amount: &Decimal) {
        self.available = (self.available - amount).round_dp(4);
    }

    fn add_held(&mut self, amount: &Decimal) {
        self.held = (self.held + amount).round_dp(4);
    }

    fn subtract_held(&mut self, amount: &Decimal) {
        self.held = (self.held - amount).round_dp(4);
    }

    fn is_tx_in_conflict(&self, tx: &u32) -> bool {
        self.transactions.contains_key(&tx)
            || self.disputes.contains_key(&tx)
            || self.handled_disputes.contains(&tx)
    }

    pub fn process_transaction(&mut self, transaction: Transaction) -> Result<(), ClientError> {
        match transaction {
            Transaction::Deposit { .. } | Transaction::Withdrawal { .. } if self.locked => {
                /*
                Client has locked account.
                */
                Err(ClientError::AccountLocked)
            }
            Transaction::Deposit { tx, .. } | Transaction::Withdrawal { tx, .. }
                if self.is_tx_in_conflict(&tx) =>
            {
                /*
                Transaction identifier detected.
                */
                Err(ClientError::TransactionReferenceConflict)
            }
            Transaction::Withdrawal { amount, .. } if self.available < amount => {
                /*
                Client has no funds for withdrawal.
                */
                Err(ClientError::InsufficientFunds)
            }
            Transaction::Deposit { tx, amount, .. } => {
                /*
                Add `amount` to available deposit.
                */
                self.add_available(&amount);
                self.transactions
                    .insert(tx, FundsTransaction::Deposit { amount });
                Ok(())
            }

            Transaction::Withdrawal { tx, amount, .. } => {
                /*
                Withdraw `amount` from available deposit.
                */
                self.subtract_available(&amount);
                self.transactions
                    .insert(tx, FundsTransaction::Withdrawal { amount });
                Ok(())
            }

            Transaction::Dispute { tx, .. } => match self.transactions.remove(&tx) {
                Some(FundsTransaction::Deposit { amount }) => {
                    /*
                    Dispute to `Deposit` transaction, move `amount` from `available` to `held`.
                    */
                    self.add_held(&amount);
                    self.subtract_available(&amount);

                    self.disputes
                        .insert(tx, FundsTransaction::Deposit { amount });
                    Ok(())
                }
                Some(FundsTransaction::Withdrawal { amount }) => {
                    /*
                    Dispute to `Withdrawal` transaction, add `amount` to `held`.
                    */
                    self.add_held(&amount);

                    self.disputes
                        .insert(tx, FundsTransaction::Withdrawal { amount });
                    Ok(())
                }
                None => Err(ClientError::InvalidTransaction),
            },
            Transaction::Resolve { tx, .. } => match self.disputes.remove(&tx) {
                Some(FundsTransaction::Deposit { amount }) => {
                    /*
                    Reject dispute of `Deposit` transaction, move `amount` from `held` to `available` .
                    */
                    self.subtract_held(&amount);
                    self.add_available(&amount);
                    self.handled_disputes.insert(tx);

                    Ok(())
                }
                Some(FundsTransaction::Withdrawal { amount }) => {
                    /*
                    Reject dispute of `Withdrawal` transaction, subtract `amount` from `held`.
                    */
                    self.subtract_held(&amount);
                    self.handled_disputes.insert(tx);

                    Ok(())
                }
                None => Err(ClientError::InvalidTransaction),
            },
            Transaction::Chargeback { tx, .. } => match self.disputes.remove(&tx) {
                Some(FundsTransaction::Deposit { amount }) => {
                    /*
                    Reverse `Deposit` transaction, subtract `amount` from `held`.
                    */
                    self.subtract_held(&amount);
                    self.handled_disputes.insert(tx);
                    self.locked = true;

                    Ok(())
                }
                Some(FundsTransaction::Withdrawal { amount }) => {
                    /*
                    Reverse of `Withdrawal` transaction, move `amount` from `held` to `available`.
                    */
                    self.subtract_held(&amount);
                    self.add_available(&amount);
                    self.handled_disputes.insert(tx);
                    self.locked = true;

                    Ok(())
                }
                None => Err(ClientError::InvalidTransaction),
            },
        }
    }
}
