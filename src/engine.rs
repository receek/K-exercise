use std::collections::HashMap;

use crate::client::Client;
use crate::transaction::Transaction;

pub struct TransactionEngine {
    pub clients: HashMap<u16, Client>,
}

impl TransactionEngine {
    pub fn new() -> Self {
        TransactionEngine {
            clients: HashMap::new(),
        }
    }

    pub fn process_transaction(&mut self, transaction: Transaction) {
        let client_id = transaction.client_id();
        let client = self
            .clients
            .entry(client_id)
            .or_insert_with(|| Client::new(client_id));
        client.process_transaction(transaction);
    }
}
