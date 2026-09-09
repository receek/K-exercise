use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::ExitCode;

use k_exercise::client::Client;
use k_exercise::engine::TransactionEngine;
use k_exercise::mmap_record_parser::MmapRecordParser;
use k_exercise::transaction::Transaction;

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

    let mut mmap_file_parser = match MmapRecordParser::new(path) {
        Ok(parser) => parser,
        Err(e) => {
            eprintln!("error: cannot load file: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut engine = match TransactionEngine::new() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("error: IO error: {e}");
            return ExitCode::FAILURE;
        }
    };

    while let Some((tx_offset, record)) = mmap_file_parser.next_record() {
        let record = match record {
            Ok(r) => r,
            Err(_e) => {
                // eprintln!("error: failed to parse record: {e}");
                continue;
            }
        };

        let transanction = match Transaction::try_from(&record) {
            Ok(v) => v,
            Err(_e) => {
                // ignore invalid records
                // eprintln!("error: cannot parse transaction from record '{record}': {e}");
                continue;
            }
        };

        if let Err(_e) = engine.process_transaction(transanction, tx_offset, &mut mmap_file_parser)
        {
            // proccesing transaction failed
            // eprintln!("error: invalid transaction: {e}");
        }
    }

    if let Err(_e) = write_clients(&engine.clients) {
        // eprintln!("error: failed to write CSV output: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
