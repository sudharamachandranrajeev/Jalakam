//! jkctl: send one control request to the POC engine and print the response.
//!
//! jkctl stats | stream-start | stream-stop | record-start | record-stop |
//!       camera <name|index|none> | mic <name|default> | shutdown

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use jkp_proto::{Envelope, Request, Response, PIPE_NAME, PROTOCOL_VERSION};

#[derive(Parser)]
struct Cli {
    command: String,
    arg: Option<String>,
    /// Retry connecting for this long (engine may still be starting).
    #[arg(long, default_value_t = 5)]
    wait_s: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let opt = |a: Option<String>| a.filter(|s| s != "none" && s != "default");
    let req = match cli.command.as_str() {
        "hello" => Request::Hello { version: PROTOCOL_VERSION },
        "stats" => Request::Stats,
        "stream-start" => Request::StreamStart,
        "stream-stop" => Request::StreamStop,
        "record-start" => Request::RecordStart,
        "record-stop" => Request::RecordStop,
        "camera" => Request::CameraSelect { device: opt(cli.arg) },
        "mic" => Request::MicSelect { device: opt(cli.arg) },
        "shutdown" => Request::Shutdown,
        other => anyhow::bail!("unknown command {other}"),
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(cli.wait_s);
    let pipe = loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
            Ok(f) => break f,
            Err(e) if std::time::Instant::now() < deadline => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e).context("connect to engine pipe"),
        }
    };
    let mut w = pipe.try_clone()?;
    let mut line = serde_json::to_vec(&Envelope { id: 1, body: req })?;
    line.push(b'\n');
    w.write_all(&line)?;
    let mut reader = BufReader::new(pipe);
    let mut resp = String::new();
    reader.read_line(&mut resp)?;
    let env: Envelope<Response> = serde_json::from_str(&resp)?;
    match env.body {
        Response::Ok { result } => {
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Response::Error { code, message } => {
            println!("{}", serde_json::json!({ "error": code, "message": message }));
            std::process::exit(2);
        }
    }
}
