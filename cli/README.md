# Frust CLI

Frust command line interface application.

## Description

**This app does not intend to interact with the web server**.

This application is designed to process your feeds configured in a `.yaml` file. It generates a static architecture in the `output` path.

I made this app because:

* I have a lot of web services in my server but in some
* I use the UI once to add feeds with filters (and make some fixes) so I had the idea to generate static contents instead and serve it with my **nginx** server

## Features

* 🧹 Clean your feeds using (include or exclude) **filters** with word(s), sentences or regex (regular expressions)
* Can **group** multiple feeds into one
* 🖼️ Retrieve article content when possible (images, audio, ...)
* 🌐 No web server: you do not need to essure you use an available network port or secure it. It generates static files so you just need to serve them in your favorite web server (mine is **nginx**)
* 🍃 Lightweight: it is written in Rust so it is green (fast and low memory usage), it also does not run in background so you just need to define a **cron** to run it periodically
* 🕵 No spyware: sources available on [GitHub](https://github.com/slundi/frust), you can check for any bloatware
* 🔒 You can run it as a non-root user. Just be sure to have permissions to write files in the output folder (mine is `/var/www/rss`)

## Configuration

Edit the [config.yaml](config.yaml) according to your needs.

## Usage

`frust-cli CONFIG_FILE`

It generates a `frust.csv` to keep track of the date when the article was added. It looks like this:

```csv
hash;ignored;date;slug
df1fd01a;false;20220214103020;my-awesome-article
```

Add it to the cron table.

## Logging

Just run: `frust-cli <arguments> > path/to/log/file.log`

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
7) [ ] Apply filters (do not match content if xpath is specified)
8)  [ ] If applicable, retrieve articles (multiple per source) and its assets if applicable
9)  [ ] Exclude already saved articles
10) [ ] Clean old articles (more than `article_keep_time` value)
11) [ ] Match remaining retrieved articles with filters
12) [ ] Generate feed files (feed, group)

## Ideas / roadmap

- [ ] Index file with links (a href and head links) to RSS feeds + errors (unreachable, ...), section with ignored article + associated filter(s)
- [ ] Filters: global named ones to use them by name on feeds/folders with a syntax of AND and OR
- [ ]  Add logger
- [ ] `produces`:
  - [ ] page with ToC if on folders or global
  - [ ] formats: HTML, Markdown, PDF, EPUB?, ZIP?, 
- [ ] `touch` article file and folder to match the feed date?
- [ ] Print help and version
- [ ] Case sensitive regex?
- [ ] Handle torrent
  - [ ] tags like the [Nyaa.si tracker](https://nyaa.si)
  - [ ] group all torrents in a single article (per feed)
- [ ] Inject data (probably links) in article
  - [ ] Download links:
    - [ ] PDF
    - [ ] ePUB?
  - [ ] Share links
- [ ] OPML Import (`-i`?) and export (`-e`?)
- [ ] Language flag
- [ ] EPG (Electronic Program Guide) support

### Dropped ideas

- Run as deamon (`-d`): use the crontab, see doc.
- Generate user in DB? prompt password? Exit without processing? (`-u USERNAME`?): Not needed, we do not use the Frust DB 
- Generate sample config? `-g path/to/FILENAME`: Not needed, we do not use the Frust DB
- Using a DB to store ignored articles, last update and hash
