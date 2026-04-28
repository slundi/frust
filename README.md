# Frust CLI

A lightweight RSS/Atom feed aggregator and filter that generates static files served by any web server.

## Why

Most self-hosted feed readers run as a persistent web service requiring a port, a database, and ongoing maintenance. Frust takes a different approach: run it from cron, write static files, serve them with Nginx. No port to expose, no daemon to babysit, and low enough resource use to run on a home router.

## Features

- **Filtering** — include or exclude articles by keyword, phrase, or regex; scoped to title, summary, or content
- **Grouping** — aggregate multiple feeds into a single output file
- **Force mode** — scrape full article body from the source page using a CSS selector when the feed only provides a summary
- **Multiple export formats** — output file format is chosen by extension:

  | Extension | Format |
  |-----------|--------|
  | `.rss` / `.xml` | RSS 2.0 |
  | `.atom` | Atom 1.0 |
  | `.json` | JSON Feed 1.1 |
  | `.md` | Markdown with YAML frontmatter |
  | `.epub` | EPUB 3.0 (grouped long-read book) |

- **Lightweight** — written in Rust; pure-Rust dependencies (no OpenSSL, no zlib); runs on ARM MUSL
- **Stateless** — designed to run via `cron`, not as a background daemon
- **Privacy** — no telemetry; runs as a non-root user; source on [Codeberg](https://codeberg.org/slundi/frust) and [GitHub](https://github.com/slundi/frust)

## Installation

### From source

```bash
cargo install --path .
```

### Cross-compile for ARM devices

It can be useful to have Frust on router or NAS (e.g. Linksys WRT1200ACS, Banana Pi R3, Helios 4, Helios64) since they
may be always on.

```bash
rustup target add armv7-unknown-linux-musleabihf
# requires arm-linux-musleabihf-gcc in PATH, or use the `cross` tool:
cargo install cross
cross build --release --target armv7-unknown-linux-musleabihf
```

The resulting binary at `target/armv7-unknown-linux-musleabihf/release/frust` is statically linked and has no runtime dependencies.

## Configuration

Copy and edit [`my-config.yaml`](my-config.yaml):

```yaml
output: /var/www/rss          # default output directory
timeout: 10                   # HTTP timeout in seconds
workers: 4                    # parallel fetch workers
retention: 30                 # days to keep articles (0 = forever)
media: false                  # download enclosures and inline images
media_max_size: 5242880       # max asset size in bytes

filters:
  - slug: no-ads
    expressions: [sponsored, advertisement]
    keep: false               # exclude matching articles

groups:
  - slug: tech
    output: /var/www/rss/tech.atom
    feeds:
      - title: "Example Blog"
        url: https://example.com/feed.xml
        filters: [no-ads]
```

See [`my-config.yaml`](my-config.yaml) for a full example with all options.

## Usage

```bash
frust path/to/config.yaml
```

Typical cron entry (every 30 minutes, log to file):

```cron
*/30 * * * * /usr/local/bin/frust /etc/frust/config.yaml >> /var/log/frust.log 2>&1
```
