use std::fmt;

use rust_decimal::Decimal;
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::engine::{Index, IndexBuffer};
use crate::mmap_record_parser::MmapRecordParser;
use crate::transaction::Transaction;

#[derive(Debug)]
pub enum ClientError {
    AccountLocked,
    IncorrectIndex,
    InsufficientFunds,
    InvalidTransaction,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::AccountLocked => write!(f, "account is locked"),
            ClientError::IncorrectIndex => write!(f, "incorrect transaction index"),
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

    /// Looks up `tx` in `index_mmap` and reads back the record it points
    /// to, returning it together with the offset it was found at. Fails
    /// with `InvalidTransaction` if `tx` has no recorded offset (or was
    /// tombstoned to `0` by a previous resolve/chargeback), if the record
    /// there can no longer be read, or if it doesn't belong to `self`.
    fn lookup_own_transaction(
        &self,
        tx: u32,
        index_mmap: &IndexBuffer,
        file_parser: &MmapRecordParser,
    ) -> Result<(Index, Transaction), ClientError> {
        let offset = index_mmap
            .get(tx)
            .filter(|&offset| offset != 0)
            .ok_or(ClientError::InvalidTransaction)?;

        let record = file_parser
            .try_read_record(offset)
            .ok_or(ClientError::InvalidTransaction)?
            .map_err(|_| ClientError::InvalidTransaction)?;

        if record.client != self.client_id {
            return Err(ClientError::InvalidTransaction);
        }

        let transaction =
            Transaction::try_from(&record).map_err(|_| ClientError::InvalidTransaction)?;
        Ok((offset, transaction))
    }

    pub fn process_transaction(
        &mut self,
        transaction: Transaction,
        tx_offset: Index,
        index_mmap: &mut IndexBuffer,
        file_parser: &mut MmapRecordParser,
    ) -> Result<(), ClientError> {
        match transaction {
            Transaction::Deposit { .. } | Transaction::Withdrawal { .. } if self.locked => {
                /*
                Client has locked account.
                */
                Err(ClientError::AccountLocked)
            }
            Transaction::Withdrawal { amount, .. } if self.available < amount => {
                /*
                Client has no funds for withdrawal.
                */
                Err(ClientError::InsufficientFunds)
            }
            Transaction::Deposit { tx, amount, .. } => {
                /*
                Record where this deposit lives in the input file, then add
                `amount` to available funds.
                */
                if !index_mmap.set(tx, tx_offset) {
                    return Err(ClientError::IncorrectIndex);
                }
                self.add_available(&amount);
                Ok(())
            }

            Transaction::Withdrawal { tx, amount, .. } => {
                /*
                Record where this withdrawal lives in the input file, then
                withdraw `amount` from available funds.
                */
                if !index_mmap.set(tx, tx_offset) {
                    return Err(ClientError::IncorrectIndex);
                }
                self.subtract_available(&amount);
                Ok(())
            }

            // `DisputedDeposit`/`DisputedWithdrawal` only ever exist as the
            // in-place, mutated form of a `Deposit`/`Withdrawal` record;
            // they can't legitimately appear as an incoming transaction.
            Transaction::DisputedDeposit { .. } | Transaction::DisputedWithdrawal { .. } => {
                Err(ClientError::InvalidTransaction)
            }

            Transaction::Dispute { tx, .. } => {
                let (offset, original) =
                    self.lookup_own_transaction(tx, index_mmap, file_parser)?;
                match original {
                    Transaction::Deposit { amount, .. } => {
                        /*
                        Dispute to `Deposit` transaction, move `amount` from `available` to `held`.
                        */
                        self.add_held(&amount);
                        self.subtract_available(&amount);
                        file_parser.set_type_first_byte(offset, b'Z');
                        Ok(())
                    }
                    Transaction::Withdrawal { amount, .. } => {
                        /*
                        Dispute to `Withdrawal` transaction, add `amount` to `held`.
                        */
                        self.add_held(&amount);
                        file_parser.set_type_first_byte(offset, b'Z');
                        Ok(())
                    }
                    // already disputed, or not a funds-moving transaction
                    _ => Err(ClientError::InvalidTransaction),
                }
            }
            Transaction::Resolve { tx, .. } => {
                let (_, disputed) = self.lookup_own_transaction(tx, index_mmap, file_parser)?;
                match disputed {
                    Transaction::DisputedDeposit { amount, .. } => {
                        /*
                        Reject dispute of `Deposit` transaction, move `amount` from `held` to `available`.
                        */
                        self.subtract_held(&amount);
                        self.add_available(&amount);
                        if !index_mmap.set(tx, 0) {
                            return Err(ClientError::IncorrectIndex);
                        }
                        Ok(())
                    }
                    Transaction::DisputedWithdrawal { amount, .. } => {
                        /*
                        Reject dispute of `Withdrawal` transaction, subtract `amount` from `held`.
                        */
                        self.subtract_held(&amount);
                        if !index_mmap.set(tx, 0) {
                            return Err(ClientError::IncorrectIndex);
                        }
                        Ok(())
                    }
                    _ => Err(ClientError::InvalidTransaction),
                }
            }
            Transaction::Chargeback { tx, .. } => {
                let (_, disputed) = self.lookup_own_transaction(tx, index_mmap, file_parser)?;
                match disputed {
                    Transaction::DisputedDeposit { amount, .. } => {
                        /*
                        Reverse `Deposit` transaction, subtract `amount` from `held`.
                        */
                        self.subtract_held(&amount);
                        self.locked = true;
                        if !index_mmap.set(tx, 0) {
                            return Err(ClientError::IncorrectIndex);
                        }
                        Ok(())
                    }
                    Transaction::DisputedWithdrawal { amount, .. } => {
                        /*
                        Reverse of `Withdrawal` transaction, move `amount` from `held` to `available`.
                        */
                        self.subtract_held(&amount);
                        self.add_available(&amount);
                        self.locked = true;
                        if !index_mmap.set(tx, 0) {
                            return Err(ClientError::IncorrectIndex);
                        }
                        Ok(())
                    }
                    _ => Err(ClientError::InvalidTransaction),
                }
            }
        }
    }
}
