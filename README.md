# Frust

A lightweight RSS feeder and aggregator.

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
* Xpath option to get article content and not brief

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
- [ ] Handle token expiration
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

## Ideas

* Check password strength/password rules
* Store articles/links (like [Wallabag](https://github.com/wallabag))
* Rename feed information
* Change feed icon
* Folder icon/glyph+color?
* Hashtag feature to find saved article
* Avoid spammers in the registration process:
  * OTP to check registration/login POST?
  * [mCaptcha](https://github.com/mCaptcha/mCaptcha/)?
  * PoW (Proof of Work) for registration/login?
* Web assembly? (to replace AJAX, for OTP, ...)
* Cache account and tokens to avoid SQL queries
