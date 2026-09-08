use std::convert::TryFrom;
use std::fmt;

use rust_decimal::Decimal;

use crate::record::Record;

#[derive(Debug)]
pub enum Transaction {
    Deposit {
        client: u16,
        tx: u32,
        amount: Decimal,
    },
    Withdrawal {
        client: u16,
        tx: u32,
        amount: Decimal,
    },
    Dispute {
        client: u16,
        tx: u32,
    },
    Resolve {
        client: u16,
        tx: u32,
    },
    Chargeback {
        client: u16,
        tx: u32,
    },
}

impl Transaction {
    pub fn client_id(&self) -> u16 {
        match self {
            Transaction::Deposit { client, .. }
            | Transaction::Withdrawal { client, .. }
            | Transaction::Dispute { client, .. }
            | Transaction::Resolve { client, .. }
            | Transaction::Chargeback { client, .. } => *client,
        }
    }
}

#[derive(Debug)]
pub enum ParseTransactionError {
    UnknownType(String),
    InvalidAmount(String),
}

impl fmt::Display for ParseTransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseTransactionError::UnknownType(t) => write!(f, "unknown transaction type: {t}"),
            ParseTransactionError::InvalidAmount(a) => write!(f, "invalid amount: {a}"),
        }
    }
}

impl std::error::Error for ParseTransactionError {}

fn parse_amount(raw: String) -> Result<Decimal, ParseTransactionError> {
    let amount: Decimal = raw
        .parse()
        .map_err(|_| ParseTransactionError::InvalidAmount(raw.clone()))?;
    if !amount.is_sign_positive() || amount.scale() > 4 {
        return Err(ParseTransactionError::InvalidAmount(raw));
    }
    Ok(amount)
}

impl TryFrom<&Record> for Transaction {
    type Error = ParseTransactionError;

    fn try_from(record: &Record) -> Result<Self, Self::Error> {
        match record.r#type.as_str() {
            "deposit" => Ok(Transaction::Deposit {
                client: record.client,
                tx: record.tx,
                amount: parse_amount(record.amount.clone())?,
            }),
            "withdrawal" => Ok(Transaction::Withdrawal {
                client: record.client,
                tx: record.tx,
                amount: parse_amount(record.amount.clone())?,
            }),
            "dispute" => Ok(Transaction::Dispute {
                client: record.client,
                tx: record.tx,
            }),
            "resolve" => Ok(Transaction::Resolve {
                client: record.client,
                tx: record.tx,
            }),
            "chargeback" => Ok(Transaction::Chargeback {
                client: record.client,
                tx: record.tx,
            }),
            other => Err(ParseTransactionError::UnknownType(other.to_string())),
        }
    }
}
