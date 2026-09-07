use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::transaction::Transaction;

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

    pub fn add_available(&mut self, amount: Decimal) {
        self.available = (self.available + amount).round_dp(4);
    }

    pub fn subtract_available(&mut self, amount: Decimal) {
        self.available = (self.available - amount).round_dp(4);
    }

    pub fn add_held(&mut self, amount: Decimal) {
        self.held = (self.held + amount).round_dp(4);
    }

    pub fn subtract_held(&mut self, amount: Decimal) {
        self.held = (self.held - amount).round_dp(4);
    }

    pub fn process_transaction(&mut self, transaction: Transaction) {
        match transaction {
            Transaction::Deposit { .. } => {}
            Transaction::Withdraw { .. } => {}
            Transaction::Dispute { .. } => {}
            Transaction::Resolve { .. } => {}
            Transaction::Chargeback { .. } => {}
        }
    }
}
