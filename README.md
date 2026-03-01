# YASTwAI

Yet Another Subtitle Translator with AI -- a command-line tool that extracts subtitles from videos and translates them using AI. Built with Rust, it preserves formatting and timing across multiple translation providers.

## Features

- **Extract and translate** -- pull subtitles from videos and translate in one step
- **Multiple providers** -- Ollama, OpenAI, Anthropic, LM Studio, vLLM
- **Parallel translation** -- concurrent batch processing with configurable parallelism
- **Context-aware** -- uses surrounding entries for consistent translations
- **Session persistence** -- resume interrupted translations automatically
- **Direct SRT translation** -- translate existing SRT files without a video source
- **Progress tracking** -- real-time progress for long translations
- **Session management** -- list, resume, and clean up translation sessions

## Requirements

- Rust 1.85+ and Cargo
- FFmpeg

## Install

```sh
git clone https://github.com/nstfn/yastwai.git
cd yastwai
cargo build --release
```

## Usage

```sh
# Copy and edit configuration
cp conf.example.json conf.json

# Translate subtitles from a video
./target/release/yastwai video.mkv

# Process all files in a directory
./target/release/yastwai videos/

# Translate an existing SRT file
./target/release/yastwai subtitles.srt

# Overwrite existing output
./target/release/yastwai -f video.mkv

# Resume an interrupted translation
./target/release/yastwai translate -R video.mkv

# Manage sessions
./target/release/yastwai sessions list
./target/release/yastwai sessions clean
```

## Configuration

Copy `conf.example.json` to `conf.json`. Key settings:

- `source_language` / `target_language` -- ISO language codes
- `translation.provider` -- which provider to use
- `translation.available_providers` -- provider-specific settings (model, endpoint, API key)

See `conf.example.json` for all options.

### Providers

| Provider | Default endpoint | Notes |
|----------|-----------------|-------|
| Ollama | `localhost:11434` | Local, no API key required |
| OpenAI | `api.openai.com` | Requires API key |
| Anthropic | `api.anthropic.com` | Requires API key |
| LM Studio | `localhost:1234` | Local, OpenAI-compatible |
| vLLM | `localhost:8000` | Local, OpenAI-compatible |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE](LICENSE).
