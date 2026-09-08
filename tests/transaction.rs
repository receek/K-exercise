use k_exercise::record::Record;
use k_exercise::transaction::{ParseTransactionError, Transaction};
use rust_decimal_macros::dec;

fn record(r#type: &str, client: u16, tx: u32, amount: &str) -> Record {
    Record {
        r#type: r#type.to_string(),
        client,
        tx,
        amount: amount.to_string(),
    }
}

#[test]
fn parses_deposit_transaction() {
    let r = record("deposit", 1, 10, "1.5000");
    let transaction = Transaction::try_from(&r).unwrap();

    match transaction {
        Transaction::Deposit { client, tx, amount } => {
            assert_eq!(client, 1);
            assert_eq!(tx, 10);
            assert_eq!(amount, dec!(1.5000));
        }
        other => panic!("expected Deposit, got {other:?}"),
    }
}

#[test]
fn parses_withdrawal_transaction() {
    let r = record("withdrawal", 2, 11, "3.25");
    let transaction = Transaction::try_from(&r).unwrap();

    match transaction {
        Transaction::Withdrawal { client, tx, amount } => {
            assert_eq!(client, 2);
            assert_eq!(tx, 11);
            assert_eq!(amount, dec!(3.25));
        }
        other => panic!("expected Withdrawal, got {other:?}"),
    }
}

#[test]
fn parses_dispute_transaction() {
    let r = record("dispute", 3, 12, "");
    let transaction = Transaction::try_from(&r).unwrap();

    match transaction {
        Transaction::Dispute { client, tx } => {
            assert_eq!(client, 3);
            assert_eq!(tx, 12);
        }
        other => panic!("expected Dispute, got {other:?}"),
    }
}

#[test]
fn parses_resolve_transaction() {
    let r = record("resolve", 4, 13, "");
    let transaction = Transaction::try_from(&r).unwrap();

    match transaction {
        Transaction::Resolve { client, tx } => {
            assert_eq!(client, 4);
            assert_eq!(tx, 13);
        }
        other => panic!("expected Resolve, got {other:?}"),
    }
}

#[test]
fn parses_chargeback_transaction() {
    let r = record("chargeback", 5, 14, "");
    let transaction = Transaction::try_from(&r).unwrap();

    match transaction {
        Transaction::Chargeback { client, tx } => {
            assert_eq!(client, 5);
            assert_eq!(tx, 14);
        }
        other => panic!("expected Chargeback, got {other:?}"),
    }
}

#[test]
fn undefined_transaction_type_returns_unknown_type_error() {
    let invalid_types = [
        "teleport",
        "this_is_a_very_long_and_completely_incorrect_transaction_type_name",
        "",
        // valid names are case-sensitive: only the exact lowercase spelling
        // is accepted.
        "Deposit",
        "WITHDRAWAL",
        "Dispute",
        "RESOLVE",
        "ChargeBack",
    ];

    for r#type in invalid_types {
        let r = record(r#type, 1, 1, "1.0");
        let err = Transaction::try_from(&r).unwrap_err();

        match err {
            ParseTransactionError::UnknownType(t) => assert_eq!(t, r#type),
            other => panic!("expected UnknownType for '{}', got {other:?}", r#type),
        }
    }
}

#[test]
fn deposit_accepts_positive_amounts_with_up_to_four_decimal_places() {
    for raw in ["1", "1.5", "1.55", "1.555", "1.5555", "0.0001"] {
        let r = record("deposit", 1, 1, raw);
        let result = Transaction::try_from(&r);
        assert!(result.is_ok(), "expected amount '{raw}' to be accepted");
    }
}

#[test]
fn deposit_rejects_amount_with_more_than_four_decimal_places() {
    let invalid_amounts = [
        "1.23456",
        // trailing zeros still count as digits behind the decimal point,
        // even though they don't add meaningful precision to the value.
        "1.20000",
        "2.100000",
        "0.00001",
    ];

    for raw in invalid_amounts {
        let r = record("deposit", 1, 1, raw);
        let err = Transaction::try_from(&r).unwrap_err();
        assert!(
            matches!(err, ParseTransactionError::InvalidAmount(_)),
            "expected amount '{raw}' to be rejected"
        );
    }
}

#[test]
fn deposit_rejects_zero_amount() {
    let r = record("deposit", 1, 1, "0");
    let err = Transaction::try_from(&r).unwrap_err();
    assert!(matches!(err, ParseTransactionError::InvalidAmount(_)));
}

#[test]
fn deposit_rejects_negative_amount() {
    let r = record("deposit", 1, 1, "-1.5");
    let err = Transaction::try_from(&r).unwrap_err();
    assert!(matches!(err, ParseTransactionError::InvalidAmount(_)));
}

#[test]
fn deposit_rejects_non_numeric_amount() {
    let r = record("deposit", 1, 1, "not-a-number");
    let err = Transaction::try_from(&r).unwrap_err();
    assert!(matches!(err, ParseTransactionError::InvalidAmount(_)));
}

#[test]
fn withdrawal_amount_validation_matches_deposit_rules() {
    // spot-check that the same amount rules apply to withdrawals, since
    // both share the same parsing logic.
    assert!(Transaction::try_from(&record("withdrawal", 1, 1, "2.1234")).is_ok());
    assert!(matches!(
        Transaction::try_from(&record("withdrawal", 1, 2, "2.12345")),
        Err(ParseTransactionError::InvalidAmount(_))
    ));
    assert!(matches!(
        Transaction::try_from(&record("withdrawal", 1, 3, "-2.0")),
        Err(ParseTransactionError::InvalidAmount(_))
    ));
    assert!(matches!(
        Transaction::try_from(&record("withdrawal", 1, 4, "0")),
        Err(ParseTransactionError::InvalidAmount(_))
    ));
}

#[test]
fn dispute_resolve_and_chargeback_ignore_amount_field_content() {
    for transaction_type in ["dispute", "resolve", "chargeback"] {
        for amount in ["", "not-a-number", "-999", "1.23456"] {
            let r = record(transaction_type, 1, 1, amount);
            let result = Transaction::try_from(&r);
            assert!(
                result.is_ok(),
                "{transaction_type} with amount '{amount}' should be accepted"
            );
        }
    }
}
