#![forbid(unsafe_code)]

use std::error::Error;

use dvs_app::{AppError, parse_args, run};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    match parse_args(args) {
        Ok(config) => {
            if let Err(error) = run(config) {
                eprintln!("{error}");
                let mut source = error.source();
                while let Some(cause) = source {
                    eprintln!("  caused by: {cause}");
                    source = cause.source();
                }
                std::process::exit(1);
            }
        }
        Err(AppError::Config(message)) if message.starts_with("Do Vale Studio 4") => {
            print!("{message}");
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
