# Agent Context: Frust CLI

## Project Overview
**Frust CLI** is a lightweight, high-performance RSS/Atom feed aggregator and filter written in Rust. It is specifically designed to run on resource-constrained environments (like routers or single-board computers like the Banana Pi R3) to generate static content that can be served via a standard web server (e.g., Nginx).

## Core Philosophy
- **Stateless & Periodic:** Designed to be run via `cron`, not as a background daemon.
- **Static Generation:** Avoids the overhead of a database-driven web UI by generating static files.
- **Resource Efficiency:** Low memory and CPU footprint (Rust). Articles are stored in `redb` (consolidated, ACID-safe). Media assets are written as flat files (`media/<xxh3>.<ext>`) so Nginx can serve them directly without any extraction layer.
- **Privacy & Security:** Runs as a non-root user; no telemetry; open-source.

## Technical Stack
- **Language:** Rust (Edition 2024)
- **Async Runtime:** `tokio`
- **Serialization/Storage:**
    - `redb`: A persistent, ACID-compliant key-value store.
    - `rkyv`: Zero-copy deserialization framework for high-speed data access.
    - `yaml-rust`: Configuration parsing.
- **Processing:**
    - `feed-rs`: Robust RSS/Atom parsing.
    - `reqwest`: HTTP client for fetching feeds and media.
    - `htmd`: HTML to Markdown conversion for article storage.
    - `twox-hash (XXH3)`: High-speed hashing for IDs and deduplication.
- **Date/Time:** `chrono`
- `my-config.yaml` is an example of the app configuration

## Architecture & Data Models
- **App:** The root configuration (timeout, workers, retention, global filters).
- **Group:** Logical aggregation of multiple feeds into a single output.
- **Feed:** Individual source metadata (URL, ETag, CSS selectors for "Force" content mode).
- **Article:** The internal data structure (XXH3 hashed ID, Markdown content, metadata).
- **Filter:** Rules-based inclusion/exclusion using Regex or exact string matching.

## Development Status (Roadmap)
### Completed
- CLI argument parsing and Config validation (YAML).
- In-memory configuration mapping (Groups/Feeds/Filters).
- Parallel asynchronous fetching of feeds.
- Basic storage logic with `redb`.

### In Progress / Planned
- **Filtering Engine:** Implementing `RegexSet` matching for titles, summaries, and content.
- **Content Enrichment:** "Force" mode to scrape full article bodies using CSS selectors.
- **Multi-format Export:** - RSS/Atom (Primary).
    - Markdown (Knowledge base integration like Obsidian).
    - EPUB (Grouped "long-read" books).
    - JSON (API-like static output).
- **Media Handling:** Configurable asset download (`media` + `media_max_size`) at app/group/feed level. Assets (enclosures and inline images) are saved to `media/<xxh3>.<ext>`, deduplicated by hash, and served directly by the web server. Retention policy cleans old entries; orphaned media files are not yet auto-purged.

## Implementation Guardrails for AI/Devs
1. **No System Dependencies for PDF:** PDF generation is explicitly excluded from the core binary. Use external tools like `pandoc` or `typst` via `std::process`.
2. **Storage split:** Articles are stored in `redb` (via `rkyv` + `zstd`) to avoid SD card fragmentation. Media assets go to flat files `media/<xxh3>.<ext>` — deduplication by hash, zero extraction cost, direct static serving by Nginx. Do **not** store media bytes in `redb`.
3. **Case Sensitivity:** Filtering defaults to Case-Insensitive to improve user experience, though regex flags can override this.
4. **I/O Optimization:** Use XXH3 for all hashing (slugs, URLs, media) to keep performance high on ARM architectures.

## Coding

After coding, alway do:
1. `cargo fmt`and `cargo clippy`. If tou have clippy warning or errors on new code, you must fix them.
2. run tests
3. suggest conventionnal commit message.

## Usage Context
```bash
frust-cli path/to/config.yaml
```
