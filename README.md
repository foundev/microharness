# microharness

A deliberately minimal agentic TUI. Think the leanest possible terminal agent
harness — single-purpose, no frameworks, no multi-provider routing. At first it
supports exactly one endpoint: an **`ollama serve`** instance.

> **What: early stage.** This repository is being initialized. The scaffold
> (Rust/Cargo), license (MIT), CI, and release-tagging are in place; the TUI
> itself is next.

## What it is

- A **Rust TUI** (`Cargo`-based, 2024 edition).
- A **client for one Ollama server endpoint** — `POST http://localhost:11434/api/chat`.

## What it is not

- Not an agent framework.
- Not multi-provider / multi-endpoint yet.
- Not an "experience" layer — no plugins, no voice, no tools beyond the chat
  round-trip.

## Current scope (v0.1.x)

- Project init: `LICENSE` (MIT), this `README`, GitHub Actions CI, and a
  release workflow are in place.
- The program is a placeholder (`main.rs` prints and exits).

## Roadmap

- [ ] Minimal chat loop — prompt in, rendered reply, exit.
- [ ] Streamed reply — consume Ollama SSE and redraw in place.
- [ ] Configurable `base_url` (default `http://localhost:11434`).

---

## Project conventions

- **CI:** GitHub Actions runs build, lint, and tests on every PR and branch
  push to `master`. See `.github/workflows/ci.yml`.
- **Releases:** cut a `vX.Y.Z` tag to trigger the release workflow and publish
  a GitHub Release. See `.github/workflows/release.yml`.
- **License:** MIT. See `LICENSE`.
- **PRs:** always target `origin`'s default branch (`master`).

## Usage (once the TUI exists)

```bash
# start Ollama locally
ollama serve

# run the TUI
cargo run
```
