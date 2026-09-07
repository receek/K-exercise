mod client;
mod engine;
mod record;
mod transaction;

use std::env;
use std::path::Path;
use std::process::ExitCode;

use record::Record;

fn load_records(path: &str) -> Result<Vec<Record>, csv::Error> {
    let mut reader = csv::Reader::from_path(path)?;
    reader.deserialize().collect()
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

    let _ = records;

    ExitCode::SUCCESS
}
