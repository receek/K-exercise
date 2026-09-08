mod client;
mod engine;
mod record;
mod transaction;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use record::Record;

use crate::client::Client;
use crate::engine::TransactionEngine;
use crate::transaction::Transaction;

fn load_records(path: &str) -> Result<Vec<Record>, csv::Error> {
    let mut reader = csv::ReaderBuilder::new()
        // remove extra whitespaces from input file
        .trim(csv::Trim::All)
        .from_path(path)?;
    reader.deserialize().collect()
}

fn write_clients(clients: &HashMap<u16, Client>) -> Result<(), Box<dyn Error>> {
    let mut writer = csv::Writer::from_writer(io::stdout());
    for client in clients.values() {
        writer.serialize(client)?;
    }
    writer.flush()?;
    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("Usage: {program} <input.csv>");
    eprintln!("  <input.csv>    path to a CSV file containing transactions");
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        print_usage(&args[0]);
        return ExitCode::FAILURE;
    }

    let path = &args[1];
    if !Path::new(path).exists() {
        eprintln!("error: file '{path}' does not exist");
        return ExitCode::FAILURE;
    }

    let records = match load_records(path) {
        Ok(records) => records,
        Err(e) => {
            eprintln!("error: failed to parse CSV file '{path}': {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut engine = TransactionEngine::new();

    for record in records.into_iter() {
        let transanction = match Transaction::try_from(record) {
            Ok(v) => v,
            _ => {
                // ignore invalid records
                continue;
            }
        };
        let _ = engine.process_transaction(transanction);
    }

    if let Err(e) = write_clients(&engine.clients) {
        eprintln!("error: failed to write CSV output: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
