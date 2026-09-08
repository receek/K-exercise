use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use k_exercise::client::Client;
use k_exercise::engine::TransactionEngine;
use k_exercise::record::Record;
use k_exercise::transaction::Transaction;

fn load_records(path: &str) -> Result<csv::DeserializeRecordsIntoIter<File, Record>, csv::Error> {
    let reader = csv::ReaderBuilder::new()
        // remove extra whitespaces from input file
        .trim(csv::Trim::All)
        .from_path(path)?;
    Ok(reader.into_deserialize())
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

    for record in records {
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: failed to parse record: {e}");
                continue;
            }
        };
        let transanction = match Transaction::try_from(&record) {
            Ok(v) => v,
            Err(e) => {
                // ignore invalid records
                eprintln!("error: cannot parse transaction from record '{record}': {e}");
                continue;
            }
        };
        if let Err(e) = engine.process_transaction(transanction) {
            // proccesing transaction failed
            eprintln!("error: invalid transaction: {e}");
        }
    }

    if let Err(e) = write_clients(&engine.clients) {
        eprintln!("error: failed to write CSV output: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
