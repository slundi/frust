# Roadmap


## Dev tasks

1) [x] Parse CLI (should be fast with just one argument)
2) [x] Check if provided config file exists
3) [x] Parse config file
   1) [x] Check for missing mandatory fields
   2) [x] Create in-memory configuration with default values
   3) [x] Replace default values
   4) [x] Check groups data
   5) [x] Check filter data
   6) [x] Match feed into the group if applicable
   7) [x] Match filters into the group if applicable
   8) [x] Match filters into the feed if applicable
5) [x] Create output file structure (`<output>/<feed slug (host like: korben.info)>/<article generated slug>`, ⚠️ filename length). Need folder if we retrieve media, otherwise a single feed file will be enough (with combined old articles with new ones)
6) [x] Retrive feeds (multiple in the same time [StackOverflow how to](https://stackoverflow.com/questions/51044467/how-can-i-perform-parallel-asynchronous-http-get-requests-with-reqwest))
7) [x] Apply filters (do not match content if `selector` is specified)
8)  [x] If applicable, retrieve articles (multiple per source) and its assets
9)  [x] Exclude already saved articles
10) [ ] Clean old articles (more than `article_keep_time` value)
11) [ ] Match remaining retrieved articles with filters
12) [x] Generate feed files (feed, group)

## Ideas / roadmap

- [ ] Index file with links (a href and head links) to RSS feeds + errors (unreachable, ...), section with ignored article + associated filter(s)
- [ ] Filters: global named ones to use them by name on feeds/folders with a syntax of AND and OR
- [ ] `exports` in the config file to have various format (global, per group or per feed or per article):
  - [ ] HTML
  - [ ] Markdown
  - [ ] EPUB
  - [ ] ~~PDF~~ to complex/heavy for the tool, use pandoc or typst
  - [ ] ZIP of JSON or markdown or HTML or XML
- [ ] Print help and version
- [ ] Handle torrent
  - [ ] tags like the [Nyaa.si tracker](https://nyaa.si)
  - [ ] group all torrents in a single article (per feed)
- [ ] OPML Import (`-i`?): generate the config file
- [ ] OPML export (`-e`?)
- [ ] Feed enrichment to inject data in an article
  - [ ] language flag? (add a flag is the feed title and `hreflang`)
  - [ ] inject HTML at top or bottop to add links to call an external API (to download the article in bookmark manager like [Shiori](https://github.com/go-shiori/shiori) or share links or ...)
  - [ ] download links so we have to export to various formats (PDF? epub?)

### Dropped ideas

- Run as deamon (`-d`): use the crontab, see doc.
- Generate user in DB? prompt password? Exit without processing? (`-u USERNAME`?): Not needed, we do not use the Frust DB
- Using a DB to store ignored articles, last update and hash
- web server
- EPG (Electronic Program Guide) support (will be a dedicated app)
- `touch` article file and folder to match the feed date: will create a lot of files so it will be IO intensive and slow when a lot a files... when save on SD card it will kill it faster, no COW (copy on write) like with redb
