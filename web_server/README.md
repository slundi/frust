# Frust WS

A lightweight RSS feeder and aggregator with web UI.

I made this project for a few reasons:

* to learn and practice RUST
* to replace my use of Feedly because I don't want to subscribe for a premium account in order to just get filters

Database is SQLite because I don't aim to host it for multiple accounts. SQLite is fast for this low traffic service.

If you want a PostgreSQL and make the project bigger, feel free to suggest a PR.

### Features

* Scroll spy in UI to mark article as read (like Feedly)
* Import/export OPML file
* Search on all feeds (to read, saved)
* Set filter to remove irrelevant feeds
* Save feed
* Folders
* Selector option to get article content and not brief

## Configuration

```ini
# Server <IP or name>:<port>
SERVER_ADDR="127.0.0.1:8330"

# Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE)
LOG_LEVEL="INFO"

# Where the SQLite database should be created/loaded
SQLITE_FILE="data/frust.sqlite3"

# Delete old (and not save from any user) articles older than XX days
ARTICLE_KEEP_TIME=30

# Where do we store feed and article assets (images for now)?
ASSETS_PATH="data/assets"

# Refresh all feed every XXX seconds
FEED_REFRESH_TIME=600

# Secret key for hashing functions
SECRET_KEY="CHANGE-ME!"

# Token duration (in days)
TOKEN_DURATION=7

# Default created folder when you register. A feed is always in a folder.
DEFAULT_FOLDER="SORT ME"
```

## Todo

### Web UI

- [ ] Skeleton
- [ ] JS
  - [x] Register (check form and display messages in case of error, send query, handle server response OK/username exists/password rules)
  - [x] Login (send query, handle wrong credential error)
  - [x] Logout
  - [x] Add/edit/remove folder
  - [ ] Add/remove feed
  - [x] Refresh token (in order to stay connected, otherwise, the server will clean token after X days of inactivity)
  - [ ] Mark article as read/saved
- [ ] Filter modal
### Server

- [x] Register
- [x] Login (create token)
- [x] Logout (delete token)
- [ ] Save user's preferences (dark mode, ...)
- [x] Handle token expiration (delete inactive every X days)
- [ ] Add feed
- [ ] Remove feed
- [ ] Refresh feed (force)
- [ ] Show article
- [ ] Mark article as read or saved (same API endpoint)
- [x] Add/edit-rename/remove folder
- [ ] Search
- [ ] Advanced search
- [ ] Download article as PDF?, HTML?, ePub?
- [ ] Export OPML

## Building

It is very simple:

1. [Install Rust](https://www.rust-lang.org/tools/install)
2. ~~Install SSL dev: `apt install libssl-dev` on Linux Debian/Ubuntu~~
3. In the folder, run the command: `cargo build --release` (see [here](https://doc.rust-lang.org/cargo/commands/cargo-build.html) for more options)

## Ideas

* Store articles/links (like [Wallabag](https://github.com/wallabag))
* Rename feed information
* Change feed icon
* Folder icon/glyph+color?
* Hashtag feature to find saved article
* Avoid spammers in the registration process:
  * OTP to check registration/login POST?
  * [mCaptcha](https://github.com/mCaptcha/mCaptcha/)?
  * PoW (Proof of Work) for registration/login? (bad idea for mobile)
* Web assembly? (to replace AJAX, for OTP, ...)
* Cache account and tokens to avoid SQL queries
* Clean unused CSS stuffs using [PurgeCSS](https://purgecss.com/)
* Replace [Bulma](https://bulma.io/) (because boilerplate around headings tags, form components, no easy dark mode) with [Tailwind CSS](https://tailwindcss.com/) or [UIkit](https://getuikit.com/)
