use std::fmt;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Record {
    #[serde(rename = "type")]
    pub r#type: String,
    pub client: u16,
    pub tx: u32,
    pub amount: String,
}

impl fmt::Display for Record {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.r#type, self.client, self.tx, self.amount
        )
    }
}
