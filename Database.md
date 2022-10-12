# Database

Database structure and documentation.

## User

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |
| **username** | `VARCHAR(32)` | Unique user name |
| **password** | `VARCHAR(96)` | Bcrypt encoded password |

Security with failed login attemps have to be handled by another system like [CrowdSec](https://www.crowdsec.net/) or fail2ban. The endpoint will returns a 401 HTTP error

## Token

Auth token that is stored in browser session/cookie in order to stay logged in and to perform modifications.

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `UUID` | Unique user toker |
| **user_id** | `INTEGER` | Foreign key to user with CASCADE |
| **created** | `DATETIME` | Date and time of creation |
| **name** | `VARCHAR(64)` | If the user wants to rename it otherwise it will use browser headers to generate it |

## Folder

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |
| **name** | VARCHAR(64) | Folder name |
| **user_id** | `INTEGER` | Foreign key to user with CASCADE |

## Filter

TODO: handle feed filters, handle global filters

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |
| **name** | VARCHAR(64) | Filter name |
| **user_id** | `INTEGER` | Foreign key to user with CASCADE |

## Feed

A fied can be common to multiple users so we don't want duplicate data.

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |
| **url** | `VARCHAR(256)` | Feed URL |
| **last_update** | `DATETIME` | Date and time when the feed has been updated |

## UserFeed

| Field | Type | Description |
|-------|:----:|-------------|
| **user_id** | `INTEGER` | Foreign key to user with CASCADE |
| **feed_id** | `INTEGER` | Foreign key to feed with CASCADE |
| **folder_id** | `INTEGER` | Optional foreign key to folder with CASCADE |
| **xpath** | `VARCHAR(256)` | Optional field to retrive feed content using HTML xpath |

## Article

An article can be common to multiple users so we don't want duplicate data

TODO
RSS: https://en.wikipedia.org/wiki/RSS
ATOM: https://www.rfc-editor.org/rfc/rfc4287.html

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |
| **link** | `VARCHAR(256)` | Feed URL |
| **title** | `VARCHAR(256)` | Article title |
| **updated** | `VARCHAR(256)` | Article date and time
| **author** | `VARCHAR(256)` |  |
| **copyright** | `VARCHAR(256)` |  |

## UserArticle

User saved articles

| Field | Type | Description |
|-------|:----:|-------------|
| **user_id** | `INTEGER` | Foreign key to user with CASCADE |
| **article_id** | `INTEGER` | Foreign key to article with CASCADE |
