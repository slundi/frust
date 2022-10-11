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

## Folder

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |
| **name** | VARCHAR(64) | Folder name |
| **user_id** | `INTEGER` | Foreign key to user with CASCADE |

## Filter

TODO

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |

## Feed

TODO

| Field | Type | Description |
|-------|:----:|-------------|
| **id** | `INTEGER` | Primary key ID |
| **slug** | `UUID` | Unique UUID that will be use in the URL |

## Article

TODO

| Field | Type | Description |
|-------|:----:|-------------|
