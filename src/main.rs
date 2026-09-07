mod client;
mod record;
mod transaction;

use std::env;
use std::path::Path;
use std::process::ExitCode;

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

    ExitCode::SUCCESS
}
