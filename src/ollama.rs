//! Minimal streaming client for a local `ollama serve` instance.
//!
//! Only the native `/api/chat` endpoint is used: send `messages` (role +
//! content pairs), stream back `{role, content}` deltas until `done`.

use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    message: Option<ChatMessage>,
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

pub struct Ollama {
    client: Client,
    base_url: String,
    model: String,
}

impl Ollama {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }

    /// Stream a single assistant reply for the given history.
    ///
    /// `messages` is a slice of `(role, content)` pairs, oldest first. Each
    /// content delta is passed to `on_token` as it arrives; the full reply is
    /// returned.
    pub async fn chat(
        &self,
        messages: &[(String, String)],
        mut on_token: impl FnMut(&str),
    ) -> Result<String> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: messages
                .iter()
                .map(|(role, content)| ChatMessage {
                    role: role.clone(),
                    content: content.clone(),
                })
                .collect(),
            stream: true,
        };

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&request)
            .send()
            .await
            .context("failed to reach ollama serve")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("ollama returned {status}: {body}"));
        }

        let mut stream = response.bytes_stream();
        let mut full = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("error reading stream")?;
            let text = String::from_utf8_lossy(&chunk);

            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let parsed: ChatChunk =
                    serde_json::from_str(line).with_context(|| format!("bad chunk: {line}"))?;

                if let Some(err) = parsed.error {
                    return Err(anyhow!(err));
                }
                if let Some(message) = parsed.message
                    && !message.content.is_empty()
                {
                    full.push_str(&message.content);
                    on_token(&message.content);
                }
                if parsed.done {
                    return Ok(full);
                }
            }
        }

        Ok(full)
    }
}
