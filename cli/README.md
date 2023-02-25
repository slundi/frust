# Frust CLI

Frust command line interface application.

## Description

**This app does not intend to interact with the web server**.

This application is designed to process your feeds configured in a `.yaml` file. It generates a static architecture in the `output` path.

I made this app because:

* I have a lot of web services in my server but in some
* I use the UI once to add feeds with filters (and make some fixes) so I had the idea to generate static contents instead and serve it with my **nginx** server

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
2) [ ] Check file exists (provided config file and data file)
3) [ ] Parse config file
   1) [ ] Check for missing mandatory fields
   2) [ ] Create in-memory configuration with default values
   3) [ ] Replace default values
   4) [ ] Check groups data
   5) [ ] Check filter data
   6) [ ] Match feed into the group if applicable
   7) [ ] Match filters into the group if applicable
   8) [ ] Match filters into the feed if applicable
5) [ ] Create output file structure (`<group slug|_ALL>/<feed slug>-<article generated slug>`, ⚠ filename length)
6) [ ] Load data file
7) [ ] Retrive feeds (multiple in the same time [StackOverflow how to](https://stackoverflow.com/questions/51044467/how-can-i-perform-parallel-asynchronous-http-get-requests-with-reqwest))
8) [ ] Check if articles are in the data file or not
9) [ ] Apply filters (do not match content if xpath is specified)
10) [ ] If applicable, retrieve articles (multiple per source) and its assets if applicable
11) [ ] Exclude already saved articles
12) [ ] Match remaining retrieved articles with filters
13) [ ] Generate feed files (feed, group)

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

## Data file

Binary formatted data `frust.dat` ([this?](https://stackoverflow.com/questions/53826371/how-to-create-a-binary-file-with-rust)):

* 32 bits (u32) xxHash to identify article
* 64 bits (u64) integer for datetime information
* 1 byte: flags
  * 0x01: ignored (by filters)
* article slug to find the output files:
  * 1 byte (u8) string lentgh
  * String depending on the previous length
* article title? (u16 + string)?
* article content? (u32 + string)?
