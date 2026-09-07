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
}
