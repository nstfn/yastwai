# YASTwAI

**Yet Another Subtitle Translator with AI** — extract subtitles from videos and translate them using AI, all from the command line.

Built in Rust. Supports local and cloud AI providers. Preserves formatting, timing, and readability.

## Features

- **One-step workflow** — extract and translate subtitles from video files or directories
- **5 AI providers** — Ollama, OpenAI, Anthropic, LM Studio, vLLM
- **Multi-pass pipeline** — analysis, translation, reflection, and validation passes
- **Quality validation** — reading speed, line length, format preservation, length ratio checks
- **Session persistence** — resume interrupted translations automatically via SQLite
- **Translation caching** — skip already-translated segments across sessions
- **Parallel processing** — configurable concurrent requests (up to 16)
- **Context-aware** — uses surrounding entries and glossary for consistent output
- **Direct SRT support** — translate existing `.srt` files without a video source

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.85+
- [FFmpeg](https://ffmpeg.org/download.html)

## Install

```sh
git clone https://github.com/nstfn/yastwai.git
cd yastwai
cargo build --release
```

The binary will be at `./target/release/yastwai`.

## Quick Start

```sh
# Copy the example config
cp conf.example.json conf.json

# Translate subtitles from a video
yastwai video.mkv

# Process a directory of videos
yastwai videos/

# Translate an existing SRT file
yastwai subtitles.srt
```

## Usage

```
yastwai [OPTIONS] <INPUT>
yastwai sessions <COMMAND>
```

| Option | Description |
|--------|-------------|
| `-p, --provider <NAME>` | AI provider (`ollama`, `openai`, `anthropic`, `lmstudio`, `vllm`) |
| `-m, --model <NAME>` | Model to use for translation |
| `-s, --source-language <CODE>` | Source language (e.g. `en`) |
| `-t, --target-language <CODE>` | Target language (e.g. `fr`) |
| `-c, --config <PATH>` | Config file path (default: `conf.json`) |
| `-f, --force-overwrite` | Overwrite existing output files |
| `-R, --resume` | Resume an interrupted translation |
| `-e, --extract-only` | Extract subtitles without translating |
| `-l, --log-level <LEVEL>` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |

### Session Management

```sh
yastwai sessions list              # List all sessions
yastwai sessions resume <ID>       # Resume a session
yastwai sessions info <ID>         # Show session details
yastwai sessions stats             # Database statistics
yastwai sessions clean             # Remove sessions older than 30 days
yastwai sessions delete <ID>       # Delete a specific session
```

## Configuration

Copy `conf.example.json` to `conf.json` and edit to taste. Key sections:

### Providers

| Provider | Endpoint | Auth |
|----------|----------|------|
| Ollama | `localhost:11434` | None |
| OpenAI | `api.openai.com` | API key |
| Anthropic | `api.anthropic.com` | API key |
| LM Studio | `localhost:1234` | None |
| vLLM | `localhost:8000` | None |

Each provider supports: `model`, `endpoint`, `concurrent_requests`, `max_chars_per_request`, `timeout_secs`, and optional `rate_limit`.

### Other Settings

- **`validation`** — toggle format, timecode, marker, and length ratio checks
- **`cache`** — in-memory and cross-session translation caching
- **`session`** — auto-resume, retention period, database path
- **`no_reflection`** — disable the AI review pass to save API calls
- **`experimental`** — opt-in features like adaptive batching, speaker tracking, glossary matching

See [`conf.example.json`](conf.example.json) for the full reference.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE) — Stefan Negouai
