// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Little script that given a input json file, prints a pretty-json to the standard output.
//! Usage: pretty-json [INPUT_JSON_FILE]
//!    or: cargo run -p pretty-json -- [INPUT_JSON_FILE]
use std::{
    fs::File,
    io::{BufReader, Result},
};

fn error(msg: &str) -> ! {
    eprintln!("Error: {msg}");
    eprintln!(
        "Usage: pretty-json [INPUT_JSON_FILE] \n   \
        or: cargo run -p pretty-json -- [INPUT_JSON_FILE]"
    );
    std::process::exit(1)
}

fn main() {
    let mut args = std::env::args();
    let filename = args.nth(1).unwrap_or_else(|| error("No argument provided"));
    pretty_json(&filename).unwrap_or_else(|err| error(&err.to_string()))
}

fn pretty_json(filename: &str) -> Result<()> {
    let input_file = File::open(&filename)?;
    let reader = BufReader::new(input_file);
    println!("Parsing {filename}");
    let value: serde_json::Value = serde_json::from_reader(reader)?;
    serde_json::to_writer_pretty(std::io::stdout(), &value)?;
    Ok(())
}
