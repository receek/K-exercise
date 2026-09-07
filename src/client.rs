use rust_decimal::Decimal;

pub struct Client {
    pub client_id: u16,
    pub available: Decimal,
    pub held: Decimal,
    pub locked: bool,
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
}
