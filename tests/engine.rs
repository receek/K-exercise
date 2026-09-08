use k_exercise::client::{Client, ClientError};
use k_exercise::engine::TransactionEngine;
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
fn deposits_are_isolated_between_clients() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(5.0)))
        .unwrap();

    let client1 = &engine.clients[&1];
    assert_eq!(client1.available, dec!(1.0));
    assert_eq!(client1.held, Decimal::ZERO);

    let client2 = &engine.clients[&2];
    assert_eq!(client2.available, dec!(5.0));
    assert_eq!(client2.held, Decimal::ZERO);
}

#[test]
fn withdrawals_are_isolated_between_clients() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(20.0)))
        .unwrap();

    engine
        .process_transaction(withdrawal(1, 3, dec!(4.0)))
        .unwrap();
    engine
        .process_transaction(withdrawal(2, 4, dec!(15.0)))
        .unwrap();

    assert_eq!(engine.clients[&1].available, dec!(6.0));
    assert_eq!(engine.clients[&2].available, dec!(5.0));
}

#[test]
fn insufficient_funds_for_one_client_does_not_affect_another() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(100.0)))
        .unwrap();

    let overdraft = engine.process_transaction(withdrawal(1, 3, dec!(50.0)));
    let ok_withdrawal = engine.process_transaction(withdrawal(2, 4, dec!(50.0)));

    assert!(matches!(overdraft, Err(ClientError::InsufficientFunds)));
    assert!(ok_withdrawal.is_ok());

    // client 1's failed withdrawal must not leak into client 2's balance.
    assert_eq!(engine.clients[&1].available, dec!(1.0));
    assert_eq!(engine.clients[&2].available, dec!(50.0));
}

#[test]
fn engine_tracks_a_separate_client_entry_per_client_id() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(1.0)))
        .unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(1.0)))
        .unwrap();
    engine
        .process_transaction(deposit(3, 3, dec!(1.0)))
        .unwrap();

    assert_eq!(engine.clients.len(), 3);
    assert!(engine.clients.contains_key(&1));
    assert!(engine.clients.contains_key(&2));
    assert!(engine.clients.contains_key(&3));
}

#[test]
fn dispute_is_isolated_to_the_client_who_owns_the_transaction() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(5.0)))
        .unwrap();

    // client 2 tries to dispute a transaction that belongs to client 1.
    let result = engine.process_transaction(dispute(2, 1));
    assert!(matches!(result, Err(ClientError::InvalidTransaction)));

    // neither client's balance is affected by the rejected dispute.
    assert_eq!(engine.clients[&1].available, dec!(5.0));
    assert_eq!(engine.clients[&1].held, Decimal::ZERO);
    assert_eq!(engine.clients[&2].available, dec!(5.0));
    assert_eq!(engine.clients[&2].held, Decimal::ZERO);
}

#[test]
fn deposit_dispute_moves_amount_from_available_to_held() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(3.0)))
        .unwrap();

    client.process_transaction(dispute(1, 1)).unwrap();

    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(3.0));
}

#[test]
fn withdrawal_dispute_adds_amount_to_held_without_touching_available() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();

    client.process_transaction(dispute(1, 2)).unwrap();

    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, dec!(4.0));
}

#[test]
fn transaction_can_be_disputed_at_most_once() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(2.0)))
        .unwrap();

    let first = client.process_transaction(dispute(1, 1));
    assert!(first.is_ok());
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(2.0));

    let second = client.process_transaction(dispute(1, 1));
    assert!(matches!(second, Err(ClientError::InvalidTransaction)));

    // the repeated dispute attempt must not change balances further.
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(2.0));
}

#[test]
fn dispute_is_allowed_even_when_account_is_locked() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    client.locked = true;

    let result = client.process_transaction(dispute(1, 1));

    assert!(result.is_ok());
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(5.0));
    assert!(client.locked);
}

#[test]
fn resolve_is_isolated_to_the_client_who_opened_the_dispute() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    engine.process_transaction(dispute(1, 1)).unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(5.0)))
        .unwrap();

    // client 2 tries to resolve a dispute that was opened by client 1.
    let result = engine.process_transaction(resolve(2, 1));
    assert!(matches!(result, Err(ClientError::InvalidTransaction)));

    // neither client's balance is affected by the rejected resolve.
    assert_eq!(engine.clients[&1].available, Decimal::ZERO);
    assert_eq!(engine.clients[&1].held, dec!(5.0));
    assert_eq!(engine.clients[&2].available, dec!(5.0));
    assert_eq!(engine.clients[&2].held, Decimal::ZERO);
}

#[test]
fn resolve_deposit_dispute_reverses_dispute_effect() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(3.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(3.0));

    client.process_transaction(resolve(1, 1)).unwrap();

    assert_eq!(client.available, dec!(3.0));
    assert_eq!(client.held, Decimal::ZERO);
}

#[test]
fn resolve_withdrawal_dispute_removes_previously_held_amount() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, dec!(4.0));

    client.process_transaction(resolve(1, 2)).unwrap();

    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, Decimal::ZERO);
}

#[test]
fn disputed_deposit_can_be_resolved_at_most_once() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(2.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();

    let first = client.process_transaction(resolve(1, 1));
    assert!(first.is_ok());
    assert_eq!(client.available, dec!(2.0));
    assert_eq!(client.held, Decimal::ZERO);

    let second = client.process_transaction(resolve(1, 1));
    assert!(matches!(second, Err(ClientError::InvalidTransaction)));

    // the repeated resolve attempt must not change balances further.
    assert_eq!(client.available, dec!(2.0));
    assert_eq!(client.held, Decimal::ZERO);
}

#[test]
fn resolve_is_allowed_even_when_account_is_locked_for_deposit_dispute() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    client.locked = true;

    let result = client.process_transaction(resolve(1, 1));

    assert!(result.is_ok());
    assert_eq!(client.available, dec!(5.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn disputed_withdrawal_can_be_resolved_at_most_once() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();

    let first = client.process_transaction(resolve(1, 2));
    assert!(first.is_ok());
    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, Decimal::ZERO);

    let second = client.process_transaction(resolve(1, 2));
    assert!(matches!(second, Err(ClientError::InvalidTransaction)));

    // the repeated resolve attempt must not change balances further.
    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, Decimal::ZERO);
}

#[test]
fn resolve_is_allowed_even_when_account_is_locked_for_withdrawal_dispute() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    client.locked = true;

    let result = client.process_transaction(resolve(1, 2));

    assert!(result.is_ok());
    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn chargeback_is_isolated_to_the_client_who_opened_the_dispute() {
    let mut engine = TransactionEngine::new();
    engine
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    engine.process_transaction(dispute(1, 1)).unwrap();
    engine
        .process_transaction(deposit(2, 2, dec!(5.0)))
        .unwrap();

    // client 2 tries to chargeback a dispute that was opened by client 1.
    let result = engine.process_transaction(chargeback(2, 1));
    assert!(matches!(result, Err(ClientError::InvalidTransaction)));

    // neither client's balance, nor its lock state, is affected by the rejected chargeback.
    assert_eq!(engine.clients[&1].available, Decimal::ZERO);
    assert_eq!(engine.clients[&1].held, dec!(5.0));
    assert!(!engine.clients[&1].locked);
    assert_eq!(engine.clients[&2].available, dec!(5.0));
    assert_eq!(engine.clients[&2].held, Decimal::ZERO);
    assert!(!engine.clients[&2].locked);
}

#[test]
fn chargeback_deposit_dispute_removes_held_amount_and_locks_account() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(3.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, dec!(3.0));

    client.process_transaction(chargeback(1, 1)).unwrap();

    // the disputed deposit is reversed for good: funds leave `held` without
    // returning to `available`.
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn chargeback_withdrawal_dispute_restores_available_and_locks_account() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    assert_eq!(client.available, dec!(6.0));
    assert_eq!(client.held, dec!(4.0));

    client.process_transaction(chargeback(1, 2)).unwrap();

    // the disputed withdrawal is reversed: its amount moves back to `available`.
    assert_eq!(client.available, dec!(10.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn disputed_deposit_can_be_chargebacked_at_most_once() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(2.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();

    let first = client.process_transaction(chargeback(1, 1));
    assert!(first.is_ok());
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);

    let second = client.process_transaction(chargeback(1, 1));
    assert!(matches!(second, Err(ClientError::InvalidTransaction)));

    // the repeated chargeback attempt must not change balances further.
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn disputed_withdrawal_can_be_chargebacked_at_most_once() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();

    let first = client.process_transaction(chargeback(1, 2));
    assert!(first.is_ok());
    assert_eq!(client.available, dec!(10.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);

    let second = client.process_transaction(chargeback(1, 2));
    assert!(matches!(second, Err(ClientError::InvalidTransaction)));

    // the repeated chargeback attempt must not change balances further.
    assert_eq!(client.available, dec!(10.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn chargeback_is_allowed_even_when_account_is_already_locked_for_deposit_dispute() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(5.0)))
        .unwrap();
    client.process_transaction(dispute(1, 1)).unwrap();
    client.locked = true;

    let result = client.process_transaction(chargeback(1, 1));

    assert!(result.is_ok());
    assert_eq!(client.available, Decimal::ZERO);
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}

#[test]
fn chargeback_is_allowed_even_when_account_is_already_locked_for_withdrawal_dispute() {
    let mut client = Client::new(1);
    client
        .process_transaction(deposit(1, 1, dec!(10.0)))
        .unwrap();
    client
        .process_transaction(withdrawal(1, 2, dec!(4.0)))
        .unwrap();
    client.process_transaction(dispute(1, 2)).unwrap();
    client.locked = true;

    let result = client.process_transaction(chargeback(1, 2));

    assert!(result.is_ok());
    assert_eq!(client.available, dec!(10.0));
    assert_eq!(client.held, Decimal::ZERO);
    assert!(client.locked);
}
