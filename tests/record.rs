use k_exercise::record::Record;

fn parse_csv(input: &str) -> Vec<Result<Record, csv::Error>> {
    let mut reader = csv::ReaderBuilder::new()
        // mirrors the reader configuration used by the binary when loading
        // the input file.
        .trim(csv::Trim::All)
        .from_reader(input.as_bytes());
    reader.deserialize::<Record>().collect()
}

#[test]
fn parses_compact_csv_without_extra_whitespace() {
    let records = parse_csv("type,client,tx,amount\ndeposit,1,1,1.5\n");

    assert_eq!(records.len(), 1);
    let record = records[0].as_ref().unwrap();
    assert_eq!(record.r#type, "deposit");
    assert_eq!(record.client, 1);
    assert_eq!(record.tx, 1);
    assert_eq!(record.amount, "1.5");
}

#[test]
fn parses_csv_with_redundant_whitespace() {
    let records = parse_csv("type, client, tx, amount\n  deposit ,  1 , 1 ,  1.5000  \n");

    assert_eq!(records.len(), 1);
    let record = records[0].as_ref().unwrap();
    assert_eq!(record.r#type, "deposit");
    assert_eq!(record.client, 1);
    assert_eq!(record.tx, 1);
    assert_eq!(record.amount, "1.5000");
}

#[test]
fn client_id_overflowing_u16_is_rejected() {
    // u16::MAX is 65535.
    let records = parse_csv("type,client,tx,amount\ndeposit,65536,1,1.5\n");

    assert_eq!(records.len(), 1);
    assert!(records[0].is_err());
}

#[test]
fn tx_id_overflowing_u32_is_rejected() {
    // u32::MAX is 4294967295.
    let records = parse_csv("type,client,tx,amount\ndeposit,1,4294967296,1.5\n");

    assert_eq!(records.len(), 1);
    assert!(records[0].is_err());
}

#[test]
fn defined_columns_are_deserialized_into_matching_fields() {
    let records = parse_csv("type,client,tx,amount\nwithdrawal,7,42,3.1400\n");

    assert_eq!(records.len(), 1);
    let record = records[0].as_ref().unwrap();
    assert_eq!(record.r#type, "withdrawal");
    assert_eq!(record.client, 7);
    assert_eq!(record.tx, 42);
    assert_eq!(record.amount, "3.1400");
}

#[test]
fn unexpected_column_name_is_rejected() {
    // "kind" does not map to any `Record` field, so the required `type`
    // column is effectively missing from the header.
    let records = parse_csv("kind,client,tx,amount\ndeposit,1,1,1.5\n");

    assert_eq!(records.len(), 1);
    assert!(records[0].is_err());
}

#[test]
fn parses_multiple_rows_with_mixed_transaction_types_and_blank_amounts() {
    let records = parse_csv(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,1.5\n",
        "  withdrawal , 1 , 2 ,  0.75  \n",
        "dispute,1,1,\n",
        "resolve,1,1,\n",
        "chargeback,2,5,\n",
    ));

    assert_eq!(records.len(), 5);
    let records: Vec<Record> = records.into_iter().map(|r| r.unwrap()).collect();

    assert_eq!(records[0].r#type, "deposit");
    assert_eq!(records[0].client, 1);
    assert_eq!(records[0].tx, 1);
    assert_eq!(records[0].amount, "1.5");

    assert_eq!(records[1].r#type, "withdrawal");
    assert_eq!(records[1].client, 1);
    assert_eq!(records[1].tx, 2);
    assert_eq!(records[1].amount, "0.75");

    // dispute/resolve/chargeback rows carry no meaningful amount, just a
    // trailing empty column.
    assert_eq!(records[2].r#type, "dispute");
    assert_eq!(records[2].amount, "");
    assert_eq!(records[3].r#type, "resolve");
    assert_eq!(records[3].amount, "");
    assert_eq!(records[4].r#type, "chargeback");
    assert_eq!(records[4].client, 2);
    assert_eq!(records[4].tx, 5);
    assert_eq!(records[4].amount, "");
}

#[test]
fn row_with_missing_trailing_column_is_rejected() {
    // the `amount` column is entirely absent from this data row (not even
    // a trailing comma for an empty value), so it has fewer fields than
    // the header declares.
    let records = parse_csv("type,client,tx,amount\ndispute,1,1\n");

    assert_eq!(records.len(), 1);
    assert!(records[0].is_err());
}
