# Frust

A lightweight RSS feeder

Lecteur de flux RSS / aggrégateur RSS avec fonctionnalités sympas

Lecteur de flux RSS avec une interface similaire à Feedly.

### Fonctionalités

* Scroll spy pour marker les élements comme lu (comme Feedly)
* Import/export OPML file
* Search on all feeds (à lire, sauvegardé)
* Set filter to remove irrelevant feeds
* Save feed
* Categories
* Xpath option to get article content and not brief

## Libraries and design

* [feed-rs](https://crates.io/crates/feed-rs)
* [opml](https://crates.io/crates/opml)
* actix
* dotenv
* [mCaptcha](https://github.com/mCaptcha/mCaptcha/)?
* PoW (Proof of Work) for registration/login?

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

# Where do we store article assets (images for now)?
ARTICLE_ASSETS_PATH="data/assets"

# Refresh all feed every XXX seconds
FEED_REFRESH_TIME=600
```

## Ideas

* Rename feed information
* Change feed icon
* Hashtag feature to find saved article
