# microharness

A deliberately minimal agentic TUI for a local [Ollama] server.

A chat interface with a status line showing the current model and reason
effort (`think`). Type a prompt and press Enter to send it; the reply streams
into the conversation.

- **MIT licensed.** Original code, permissively-licensed dependencies only.
- **One endpoint.** `http://localhost:11434` (Ollama `serve`).
- **Small surface.** A streaming chat TUI and nothing more.

## Usage

Start Ollama locally, then run:

```bash
ollama serve
cargo run                           # defaults: localhost:11434, llama3.2
cargo run -- --model qwen3
cargo run -- --base-url http://127.0.0.1:11434
```

In the TUI:

- Type a prompt and press **Enter** to send it.
- Press **Ctrl-M** to open the model picker (lists models via `/api/tags`; `↑/↓` navigate, `Enter` select, `Esc` close).
- Press **Ctrl-T** to cycle the think level: `auto → on → off → low → medium → high → max`.
- Press **Ctrl-C** to quit.

The current model, server, and think level are shown in the status line at the
bottom.

## Config

- `--base-url` — Ollama server base URL (default `http://localhost:11434`).
- `--model` — model name (default `llama3.2`; pick one you've pulled, e.g.
  `qwen3` for a reasoning model).

## Project conventions

- **License:** MIT (`LICENSE`).
- **CI:** GitHub Actions runs `cargo fmt --check`, `cargo clippy`, build, and
  tests on every PR and push to `master` (`.github/workflows/ci.yml`).
- **Releases:** cut a `vX.Y.Z` tag to trigger release packaging + a GitHub
  Release (`.github/workflows/release.yml`).
- **PRs:** target `origin/master`, the default branch.

## Roadmap

- [x] Streamed chat loop against Ollama `/api/chat`
- [x] TUI with model/think status line, keyboard-driven
- [ ] Conversation persistence / session resume
- [ ] Selectable streaming (accumulate vs. redraw)

[Ollama]: https://github.com/ollama/ollama
