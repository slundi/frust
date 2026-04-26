# Agent Context: Frust CLI

## Project Overview
**Frust CLI** is a lightweight, high-performance RSS/Atom feed aggregator and filter written in Rust. It is specifically designed to run on resource-constrained environments (like routers or single-board computers like the Banana Pi R3) to generate static content that can be served via a standard web server (e.g., Nginx).

## Core Philosophy
- **Stateless & Periodic:** Designed to be run via `cron`, not as a background daemon.
- **Static Generation:** Avoids the overhead of a database-driven web UI by generating static files.
- **Resource Efficiency:** Low memory and CPU footprint (Rust). Minimal Disk I/O impact by preferring consolidated storage (`redb`) over thousands of small files.
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
- **Media Handling:** Local enclosure retrieval (images/audio) with hashed deduplication.

## Implementation Guardrails for AI/Devs
1. **No System Dependencies for PDF:** PDF generation is explicitly excluded from the core binary. Use external tools like `pandoc` or `typst` via `std::process`.
2. **Binary Storage:** Articles and Media should be stored in `redb` using `rkyv` and `zstd` compression (for articles) to prevent SD card wear and file system fragmentation.
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
