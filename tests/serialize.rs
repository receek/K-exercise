use k_exercise::client::Client;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Serializes `client` the same way the binary does (through a `csv::Writer`)
/// and returns its `(client, available, held, total, locked)` fields as
/// strings, as they would appear in the output CSV row.
fn serialize_client(client: &Client) -> (String, String, String, String, String) {
    let mut writer = csv::Writer::from_writer(vec![]);
    writer.serialize(client).unwrap();
    let output = String::from_utf8(writer.into_inner().unwrap()).unwrap();

    let mut lines = output.lines();
    assert_eq!(lines.next().unwrap(), "client,available,held,total,locked");
    let fields: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert_eq!(fields.len(), 5);

    (
        fields[0].to_string(),
        fields[1].to_string(),
        fields[2].to_string(),
        fields[3].to_string(),
        fields[4].to_string(),
    )
}

#[test]
fn serialized_total_equals_available_plus_held() {
    let cases = [
        (dec!(0), dec!(0)),
        (dec!(100), dec!(0)),
        (dec!(1.5), dec!(2.25)),
        (dec!(-10.5), dec!(3.25)),
        (dec!(999.9999), dec!(0.0001)),
        (dec!(-5.0), dec!(-5.0)),
    ];

    for (id, (available, held)) in cases.into_iter().enumerate() {
        let mut client = Client::new(id as u16);
        client.available = available;
        client.held = held;

        let (_, available_field, held_field, total_field, _) = serialize_client(&client);

        assert_eq!(available_field.parse::<Decimal>().unwrap(), available);
        assert_eq!(held_field.parse::<Decimal>().unwrap(), held);
        assert_eq!(
            total_field.parse::<Decimal>().unwrap(),
            available + held,
            "total should equal available + held for client {id}"
        );
    }
}

#[test]
fn serializes_available_balance_with_various_decimal_precisions() {
    let values = [dec!(0), dec!(5), dec!(5.1), dec!(5.12), dec!(5.123), dec!(5.1234)];

    for (id, available) in values.into_iter().enumerate() {
        let mut client = Client::new(id as u16);
        client.available = available;

        let (_, available_field, ..) = serialize_client(&client);
        assert_eq!(available_field, available.to_string());
    }
}

#[test]
fn serializes_held_balance_with_various_decimal_precisions() {
    let values = [dec!(0), dec!(5), dec!(5.1), dec!(5.12), dec!(5.123), dec!(5.1234)];

    for (id, held) in values.into_iter().enumerate() {
        let mut client = Client::new(id as u16);
        client.held = held;

        let (_, _, held_field, ..) = serialize_client(&client);
        assert_eq!(held_field, held.to_string());
    }
}

#[test]
fn serializes_negative_available_balance_correctly() {
    // `available` can legitimately go negative (see README), so its sign
    // must round-trip through serialization exactly.
    let values = [
        dec!(-1),
        dec!(-0.5),
        dec!(-1234.5678),
        dec!(-0.0001),
        dec!(-100),
    ];

    for (id, available) in values.into_iter().enumerate() {
        let mut client = Client::new(id as u16);
        client.available = available;

        let (_, available_field, .., total_field, _) = serialize_client(&client);
        assert_eq!(available_field, available.to_string());
        assert!(available_field.starts_with('-'));
        assert_eq!(total_field.parse::<Decimal>().unwrap(), available);
    }
}

#[test]
fn serializes_locked_flag_correctly() {
    let mut unlocked = Client::new(1);
    unlocked.locked = false;
    let (.., locked_field) = serialize_client(&unlocked);
    assert_eq!(locked_field, "false");

    let mut locked = Client::new(2);
    locked.locked = true;
    let (.., locked_field) = serialize_client(&locked);
    assert_eq!(locked_field, "true");
}

#[test]
fn serializes_client_id_correctly() {
    for id in [0u16, 1, 42, 1000, u16::MAX] {
        let client = Client::new(id);

        let (client_field, ..) = serialize_client(&client);
        assert_eq!(client_field, id.to_string());
    }
}
