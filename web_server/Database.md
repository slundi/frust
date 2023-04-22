# Database

Database structure and documentation.

## Account

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **username** | `VARCHAR(32)` | Unique account name |
| **password** | `VARCHAR(96)` | Bcrypt encoded password |
| **config** | `TEXT` | JSON parsed account preferences |

Security with failed login attemps have to be handled by another system like [CrowdSec](https://www.crowdsec.net/) or fail2ban. The endpoint will returns a 401 HTTP error

## Token

Auth token that is stored in browser session/cookie in order to stay logged in and to perform modifications.

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `HashID` | Unique account toker |
| **account_id** | `INTEGER` | Foreign key to account with CASCADE |
| **created** | `DATETIME` | Date and time of creation |
| **name** | `VARCHAR(64)` | If the account wants to rename it otherwise it will use browser headers to generate it |

## Folder

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **name** | VARCHAR(64) | Folder name |
| **account_id** | `INTEGER` | Foreign key to account with CASCADE |

## Filter

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **name** | `VARCHAR(64)` | Filter name |
| **account_id** | `INTEGER` | Foreign key to account with CASCADE |
| **subscription_id** | `INTEGER` | Foreign key to the subscription, `NULL` for global filters |
| **find** | `VARCHAR(256)` | Text to find |
| **is_regex** | `BOOLEAN` | if the find string is a regular expression |
| **in_title** | `BOOLEAN` | if we search in the title |
| **in_content** | `BOOLEAN` | if we search in the content |
| **includes** | `BOOLEAN` | If we only want to include results that match this filter (or other include filters). If false, results from this filter are excluded. |

## Feed

A fied can be common to multiple accounts so we don't want duplicate data.

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **url** | `VARCHAR(256)` | Feed URL |
| **last_update** | `DATETIME` | Date and time when the feed has been updated |

Icon will be save on drive

## Subscription

| Field | Type | Description |
|-------|:----:|-------------|
| **account_id** | `INTEGER` | Foreign key to account with CASCADE |
| **feed_id** | `INTEGER` | Foreign key to feed with CASCADE |
| **folder_id** | `INTEGER` | Optional foreign key to folder with CASCADE |
| **selector** | `VARCHAR(256)` | Optional field to retrive feed content using HTML CSS selector |
| **added** | `DATETIME` | When the feed was added to the account |

## Article

An article can be common to multiple accounts so we don't want duplicate data. They are kept an amount of time depending on the server configuration. Saved articles won't be deleted.

TODO: group [RSS](https://en.wikipedia.org/wiki/RSS) and [ATOM](https://www.rfc-editor.org/rfc/rfc4287.html) contents

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **link** | `VARCHAR(256)` | Feed URL |
| **title** | `VARCHAR(256)` | Article title |
| **updated** | `VARCHAR(256)` | Article date and time
| **author** | `VARCHAR(256)` |  |
| **copyright** | `VARCHAR(256)` |  |

## accountArticle

account saved and read articles

| Field | Type | Description |
|-------|:----:|-------------|
| **account_id** | `INTEGER` | Foreign key to account with CASCADE |
| **article_id** | `INTEGER` | Foreign key to article with CASCADE |
| **saved** | `DATETIME` | Date when the article was saved. Default `NULL` means the article was read. |
