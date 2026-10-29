# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

mdterm is a terminal-based Markdown viewer written in Rust. It renders Markdown files with syntax highlighting, styled formatting, and interactive navigation (scrolling, keyboard/mouse controls). When stdout is piped, it outputs plain styled text instead of the interactive TUI.

## Build Commands

```bash
cargo build              # debug build
cargo build --release    # release build
cargo run -- <file.md>   # run with a markdown file
cargo check              # type-check without building
cargo clippy             # lint
cargo fmt                # format code
cargo test               # run tests
cargo build --no-default-features  # build without POS highlighting
```

The `pos` feature (part-of-speech highlighting) is on by default; `--no-default-features` builds a lean binary without the embedded model.

## Architecture

Eleven source files in `src/`:

- **main.rs** — Entry point. Uses `clap` for CLI arg parsing, handles stdin/file input, dispatches to viewer (TTY), piped output, or HTML export.
- **markdown.rs** — Stateful markdown renderer. Processes `pulldown-cmark` events into `(Vec<Line>, DocumentInfo)`. Handles syntax highlighting, math rendering (LaTeX→Unicode), image placeholders, line numbers, mermaid diagram rendering (as generated images), and metadata tracking (headings, code blocks, slide breaks).
- **style.rs** — Data types (`Style`, `StyledSpan`, `Line`, `LineMeta`, `DocumentInfo`) and word-wrapping logic. `LineMeta` tracks heading/code-block/slide metadata through wrapping.
- **viewer.rs** — Interactive TUI with multiple view modes (Normal, Search, TOC, LinkPicker, FuzzyHeading). Supports slide mode, auto-reload on file changes (via notify crate), multi-file switching, clipboard operations, regex search, overlay panels, and image rendering.
- **theme.rs** — Two complete themes (dark/light) with 40+ color fields including overlay, math, image, and line number colors.
- **config.rs** — Loads `~/.config/mdterm/config.toml` for persistent settings (theme, line_numbers, width).
- **export.rs** — HTML export with inline CSS matching the current theme.
- **image.rs** — Terminal image rendering with three protocols: Kitty (ID-based upload/placement), iTerm2 (inline image sequences), and Unicode half-block fallback. Fetches images on background threads via `std::sync::mpsc` (non-blocking — `start_fetch()` spawns a thread per URL, `poll_completed()` drains results each event-loop tick). Also handles downscaling, caching, and terminal cell metric detection.
- **json.rs** — JSON file viewer. Parses JSON and renders it with semantic coloring (keys, strings, numbers, booleans, nulls) and indented structure.
- **diagram.rs** — Mermaid-to-PNG image rendering (via `merman` crate) plus the shared `Canvas`/`CardDrawRow` drawing primitives used by the JSON diagram mode. The old ASCII art renderer has been removed.
- **pos.rs** — Part-of-speech (POS) highlighting, behind the `pos` cargo feature (on by default). A vendored averaged-perceptron `PosTagger` (embedded gzip model) tags prose words; `apply()` recolors prose spans by category (noun/verb/adjective/…). Exempts inline code, links, code blocks, and YAML frontmatter.

**Data flow:** markdown text → `pulldown-cmark` events → `Renderer` (markdown.rs) → `(Vec<Line>, DocumentInfo)` → `wrap_lines` (style.rs) → terminal/HTML output

## Key Dependencies

- **pulldown-cmark 0.11** — CommonMark parser (events/AST, math support)
- **crossterm 0.28** — Terminal control (raw mode, colors, events)
- **syntect 5** — Syntax highlighting for code blocks
- **clap 4** — CLI argument parsing
- **regex 1** — Regex search support
- **open 5** — Open URLs in browser (link picker)
- **serde + toml** — Config file parsing
- **dirs 5** — Platform config directory lookup
- **image 0.25** — Image loading and processing (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF)
- **libc 0.2** — Unix FFI for terminal cell pixel metrics (ioctl TIOCGWINSZ)
- **base64 0.22** — Base64 encoding for image protocol escape sequences
- **ureq 3** — Pure-Rust HTTP client for fetching remote images (replaces shelling out to `curl`)
- **serde_json 1** — JSON parsing for the JSON file viewer
- **notify 7** — Cross-platform filesystem watcher (inotify/FSEvents/kqueue) for auto-reload
- **flate2 1** — gzip decode for the embedded POS model (optional, `pos` feature)

## Rust Edition

Uses Rust edition 2024 (requires rustc 1.85+).
