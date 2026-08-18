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

/// Reason-effort control passed to Ollama's `think` field.
///
/// Ollama accepts either a boolean (`think: true`/`false`) or a level string
/// (`"low"`, `"medium"`, `"high"`, `"max"`). It serializes to that JSON value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Think {
    Auto,
    On,
    Off,
    Low,
    Medium,
    High,
    Max,
}

impl Think {
    pub fn as_json(self) -> serde_json::Value {
        use Think::*;
        match self {
            Auto => serde_json::Value::Bool(true),
            On => serde_json::Value::Bool(true),
            Off => serde_json::Value::Bool(false),
            Low => serde_json::Value::String("low".into()),
            Medium => serde_json::Value::String("medium".into()),
            High => serde_json::Value::String("high".into()),
            Max => serde_json::Value::String("max".into()),
        }
    }

    /// Human label for the status line.
    pub fn label(self) -> &'static str {
        use Think::*;
        match self {
            Auto => "auto",
            On => "on",
            Off => "off",
            Low => "low",
            Medium => "medium",
            High => "high",
            Max => "max",
        }
    }

    /// Next level when the user cycles.
    pub fn next(self) -> Self {
        use Think::*;
        match self {
            Auto => On,
            On => Off,
            Off => Low,
            Low => Medium,
            Medium => High,
            High => Max,
            Max => Auto,
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    message: Option<ChatMessage>,
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone)]
pub struct Ollama {
    client: Client,
    base_url: String,
    model: String,
    think: Think,
}

impl Ollama {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self::with_think(base_url, model, Think::Auto)
    }

    pub fn with_think(base_url: &str, model: &str, think: Think) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            think,
        }
    }

    pub fn set_think(&mut self, think: Think) {
        self.think = think;
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
            think: Some(self.think.as_json()),
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
        // Buffered bytes that may span HTTP chunk boundaries. Records are only
        // parsed once a full `\n`-terminated line is available, so a chunk
        // boundary can never split a UTF-8 character or a JSON record.
        let mut buffer: Vec<u8> = Vec::new();

        let mut handle_line = |line: &str| -> Result<bool> {
            let line = line.trim();
            if line.is_empty() {
                return Ok(false);
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
            Ok(parsed.done)
        };

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("error reading stream")?;
            buffer.extend_from_slice(&chunk);

            for line in take_complete_lines(&mut buffer) {
                let text = std::str::from_utf8(&line).context("stream contained invalid UTF-8")?;
                if handle_line(text)? {
                    return Ok(full);
                }
            }
        }

        // Flush a trailing record that arrived without a final newline.
        for line in take_remaining(&mut buffer) {
            let text = std::str::from_utf8(&line).context("stream contained invalid UTF-8")?;
            handle_line(text)?;
        }

        Ok(full)
    }

    /// List the model names available on the server (`GET /api/tags`).
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .context("failed to list models")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("ollama returned {status}: {body}"));
        }

        #[derive(serde::Deserialize)]
        struct Tags {
            models: Vec<Model>,
        }
        #[derive(serde::Deserialize)]
        struct Model {
            name: String,
        }

        let tags: Tags = response.json().await.context("bad /api/tags response")?;
        let mut names: Vec<String> = tags.models.into_iter().map(|m| m.name).collect();
        names.sort();
        Ok(names)
    }
}

/// Drain every `\n`-terminated record from `buffer`, leaving any partial
/// trailing record in place. Returns complete records in order.
///
/// Records are only returned once a full newline is present, so a chunk
/// boundary can never split a JSON record or a multi-byte UTF-8 character.
fn take_complete_lines(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
        lines.push(buffer.drain(..=pos).collect());
    }
    lines
}

/// Drain any remaining bytes in `buffer` as a final record, clearing the
/// buffer. Used only after the stream has ended to capture a trailing record
/// that arrived without a newline.
fn take_remaining(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    if buffer.is_empty() {
        return Vec::new();
    }
    vec![std::mem::take(buffer)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The buffer splitter must not emit a partial JSON record or a partial
    /// multi-byte UTF-8 character: records are only returned once a full
    /// newline is present.
    #[test]
    fn buffers_records_across_chunk_boundaries() {
        // One JSON record with a multi-byte UTF-8 char (U+270B "✋") inside.
        // The UTF-8 bytes are spliced in so the byte literal stays ASCII.
        let head = br#"{"message":{"role":"assistant","content":"hi "#;
        let waved_hand = "\u{270B}";
        let tail = br#""},"done":false}"#;

        let mut record = head.to_vec();
        record.extend_from_slice(waved_hand.as_bytes());
        record.extend_from_slice(tail);
        let mut with_nl = record.clone();
        with_nl.push(b'\n');

        // Feed the record in pieces that split the UTF-8 char and the newline.
        let split = 7; // inside "content" value bytes, before the multi-byte char start
        let split2 = with_nl.len() - 1; // newline is the only byte in the final chunk

        let mut buffer = Vec::new();
        buffer.extend_from_slice(&with_nl[..split]);
        assert!(
            take_complete_lines(&mut buffer).is_empty(),
            "no complete line yet"
        );

        buffer.extend_from_slice(&with_nl[split..split2]);
        assert!(
            take_complete_lines(&mut buffer).is_empty(),
            "still no newline"
        );

        buffer.push(with_nl[split2]); // now the newline arrives
        let lines = take_complete_lines(&mut buffer);
        assert_eq!(lines.len(), 1, "exactly one complete record");
        assert_eq!(&lines[0], &with_nl);
    }

    /// A trailing record without a final newline is still surfaced once the
    /// stream ends.
    #[test]
    fn flushes_trailing_record_without_newline() {
        let mut buffer = "partial".as_bytes().to_vec();
        assert!(take_complete_lines(&mut buffer).is_empty());
        let rest = take_remaining(&mut buffer);
        assert_eq!(rest, vec![b"partial".to_vec()]);
        assert!(buffer.is_empty());
    }
}
