# microharness

A deliberately minimal agentic harness for a local [Ollama] server.

It reads prompts from stdin, streams replies from a model over Ollama's native
`/api/chat` endpoint, and keeps a multi-turn conversation going until EOF
(Ctrl-D).

- **MIT licensed.** Original code, permissively-licensed dependencies only.
- **One endpoint.** `http://localhost:11434` (Ollama `serve`).
- **No TUI library, no agent framework.** A streamed chat REPL. That's the
  whole point.

## Usage

Start Ollama locally, then run:

```bash
ollama serve
cargo run                          # defaults: localhost:11434, llama3.2
cargo run --model qwen2.5
cargo run --base-url http://127.0.0.1:11434 --model mixtral
```

Type a prompt and press Enter; the reply streams in place. Ctrl-D exits.

## Config

- `--base-url` — Ollama server base URL (default `http://localhost:11434`).
- `--model` — model name (default `llama3.2`; pick one you've pulled).

## Project conventions

- **License:** MIT (`LICENSE`).
- **CI:** GitHub Actions runs `cargo fmt --check`, `cargo clippy`, build, and
  tests on every PR and push to `master` (`.github/workflows/ci.yml`).
- **Releases:** cut a `vX.Y.Z` tag to trigger release packaging + a GitHub
  Release (`.github/workflows/release.yml`).
- **PRs:** target `origin/master`, the default branch.

## Roadmap

- [x] Streamed chat loop against Ollama `/api/chat`
- [ ] Conversation persistence / session resume
- [ ] Selectable streaming (accumulate vs. redraw)

[Ollama]: https://github.com/ollama/ollama
