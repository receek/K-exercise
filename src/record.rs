use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Record {
    #[serde(rename = "type")]
    pub r#type: String,
    pub client: u16,
    pub tx: u32,
    pub amount: String,
}
