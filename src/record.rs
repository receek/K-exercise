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

impl Record {
    /// Deserializes a single CSV record (no header row of its own) from raw
    /// bytes, mapping its columns onto `Record`'s fields by name using
    /// `headers` — so the input's columns may appear in any order.
    pub fn from_bytes(bytes: &[u8], headers: &csv::ByteRecord) -> Result<Self, csv::Error> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .trim(csv::Trim::All)
            .from_reader(bytes);

        let mut record = csv::ByteRecord::new();
        reader.read_byte_record(&mut record)?;
        record.deserialize(Some(headers))
    }
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
