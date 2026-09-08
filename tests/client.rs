use k_exercise::client::{Client, ClientError};
use k_exercise::transaction::Transaction;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn deposit(client: u16, tx: u32, amount: Decimal) -> Transaction {
    Transaction::Deposit { client, tx, amount }
}

fn withdrawal(client: u16, tx: u32, amount: Decimal) -> Transaction {
    Transaction::Withdrawal { client, tx, amount }
}

fn dispute(client: u16, tx: u32) -> Transaction {
    Transaction::Dispute { client, tx }
}

fn resolve(client: u16, tx: u32) -> Transaction {
    Transaction::Resolve { client, tx }
}

fn chargeback(client: u16, tx: u32) -> Transaction {
    Transaction::Chargeback { client, tx }
}

#[test]
fn several_deposits_accumulate_for_single_client() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    client
        .process_transaction(deposit(1, 2, dec!(2.5)))
        .unwrap();
    client
        .process_transaction(deposit(1, 3, dec!(0.2500)))
        .unwrap();
    assert_eq!(client.available, dec!(3.7500));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(!client.locked);
}

#[test]
fn new_client_has_zeroed_state() {
    let client = Client::new(1);
    assert_eq!(client.client_id, 1);
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, Decimal::ZERO);
    assert!(!client.locked);
}

#[test]
fn deposit_increases_available() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(1.5)))
        .unwrap();
    assert_eq!(client.available, dec!(1.5));
    assert_eq!(client.held, Decimal::ZERO);
}

#[test]
fn withdrawal_decreases_available() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(2.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(0.5)))
        .unwrap();
    assert_eq!(client.available, dec!(1.5));
}

#[test]
fn withdrawal_fails_with_insufficient_funds() {
    let mut client = Client::new(1);
    let result = client.process_transaction(withdrawal(1, 1, dec!(1.0)));
    assert!(matches!(result, Err(ClientError::InsufficientFunds)));
    assert_eq!(client.available, Decimal::ZERO);
}

#[test]
fn deposit_rejected_when_locked() {
    let mut client = Client::new(1);
    client.locked = true;
    let result = client.process_transaction(deposit(1, 1, dec!(1.0)));
    assert!(matches!(result, Err(ClientError::AccountLocked)));
    assert_eq!(client.available, Decimal::ZERO);
}

#[test]
fn duplicate_tx_id_is_rejected() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    let result = client.process_transaction(deposit(1, 1, dec!(2.0)));
    assert!(matches!(
        result,
        Err(ClientError::TransactionReferenceConflict)
    ));
    assert_eq!(client.available, dec!(1.0));
}

#[test]
fn dispute_deposit_moves_funds_to_held() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(1.0));
}

#[test]
fn dispute_withdrawal_holds_amount_without_touching_available() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(2.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    assert_eq!(client.available, dec!(3.0));
    assert_eq!(client.held, dec!(2.0));
}

#[test]
fn dispute_unknown_tx_is_invalid() {
    let mut client = Client::new(1);
    let result = client.process_transaction(dispute(1, 99));
    assert!(matches!(result, Err(ClientError::InvalidTransaction)));
}

#[test]
fn resolve_deposit_dispute_returns_funds_to_available() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    client.process_transaction(resolve(1, 1)).unwrap();
    assert_eq!(client.available, dec!(1.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(!client.locked);
}

#[test]
fn resolve_withdrawal_dispute_releases_held_amount() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(2.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    client.process_transaction(resolve(1, 2)).unwrap();
    assert_eq!(client.available, dec!(3.0));
    assert_eq!(client.held, Decimal::ZERO);
}

#[test]
fn resolve_unknown_tx_is_invalid() {
    let mut client = Client::new(1);
    let result = client.process_transaction(resolve(1, 99));
    assert!(matches!(result, Err(ClientError::InvalidTransaction)));
}

#[test]
fn chargeback_deposit_dispute_locks_account_and_removes_held_funds() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    client.process_transaction(chargeback(1, 1)).unwrap();
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn chargeback_withdrawal_dispute_locks_account_and_restores_available() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(2.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    client.process_transaction(chargeback(1, 2)).unwrap();
    assert_eq!(client.available, dec!(5.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn chargeback_unknown_tx_is_invalid() {
    let mut client = Client::new(1);
    let result = client.process_transaction(chargeback(1, 99));
    assert!(matches!(result, Err(ClientError::InvalidTransaction)));
}
