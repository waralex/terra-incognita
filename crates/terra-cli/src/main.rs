use std::io::{self, Read};
use std::process;

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "http://localhost:3000/query".to_string());

    let mut body = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut body) {
        eprintln!("error reading stdin: {e}");
        process::exit(1);
    }

    if body.trim().is_empty() {
        eprintln!("error: empty input");
        process::exit(1);
    }

    let client = reqwest::blocking::Client::new();
    let resp = match client
        .post(&url)
        .header("content-type", "application/yaml")
        .body(body)
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    let status = resp.status();
    let text = resp.text().unwrap_or_default();

    print!("{text}");

    if !status.is_success() {
        process::exit(1);
    }
}
