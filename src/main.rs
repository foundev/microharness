//! microharness — a minimal agentic harness for local Ollama serve.
//!
//! Reads prompts from stdin, streams replies from the model, and keeps a
//! multi-turn conversation going until EOF (Ctrl-D).

mod ollama;

use anyhow::Result;
use ollama::Ollama;
use std::io::{BufRead, Write};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "llama3.2";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut base_url = DEFAULT_BASE_URL.to_string();
    let mut model = DEFAULT_MODEL.to_string();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--base-url" => {
                i += 1;
                base_url = args.get(i).cloned().unwrap_or_default();
            }
            "--model" => {
                i += 1;
                model = args.get(i).cloned().unwrap_or_default();
            }
            other => return Err(anyhow::anyhow!("unknown argument: {other}")),
        }
        i += 1;
    }

    let client = Ollama::new(&base_url, &model);
    let stdin = std::io::stdin();
    let mut history: Vec<(String, String)> = Vec::new();

    eprintln!("microharness: talking to {model} at {base_url}");
    eprintln!("type a prompt (Ctrl-D to exit)");

    for line in stdin.lock().lines() {
        let line = line?;
        let prompt = line.trim();
        if prompt.is_empty() {
            continue;
        }

        history.push(("user".to_string(), prompt.to_string()));
        let reply = client
            .chat(&history, |delta| {
                print!("{delta}");
                let _ = std::io::stdout().flush();
            })
            .await?;
        println!();
        history.push(("assistant".to_string(), reply));
    }

    Ok(())
}
