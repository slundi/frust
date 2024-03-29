CREATE TABLE IF NOT EXISTS account (
	id  INTEGER NOT NULL PRIMARY KEY,
	username  VARCHAR(32) NOT NULL,
	encrypted_password   VARCHAR(72) NOT NULL,
	created DATETIME NOT NULL DEFAULT(DATETIME('now')),
	config TEXT NOT NULL DEFAULT '{}',
	UNIQUE(username)
);

CREATE TABLE IF NOT EXISTS token (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	created DATETIME NOT NULL DEFAULT(DATETIME('now')),
	value VARCHAR(64),
	UNIQUE(value)
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS folder (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	name VARCHAR(64) NOT NULL COLLATE NOCASE,
	UNIQUE(account_id, name)
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS feed (
	id  INTEGER NOT NULL PRIMARY KEY,
	title VARCHAR(255) NOT NULL,
	feed_link VARCHAR(255) NOT NULL,
	page_link VARCHAR(255),
	status VARCHAR(16) NOT NULL DEFAULT('ADDED'),
	description TEXT NOT NULL DEFAULT(''),	
	language TEXT,
	updated DATETIME,
	extra TEXT,
	UNIQUE(feed_link)
);

CREATE TABLE IF NOT EXISTS subscription (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	feed_id INTEGER NOT NULL,
	folder_id INTEGER,
	name VARCHAR(255) COLLATE NOCASE, -- feed name if the user rename it
	selector VARCHAR(255),
	added DATETIME NOT NULL DEFAULT(DATETIME('now')),
	inject BOOLEAN NOT NULL DEFAULT(TRUE),
	UNIQUE(account_id, feed_id),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
	FOREIGN KEY(feed_id) REFERENCES feed(id) ON DELETE CASCADE
	FOREIGN KEY(folder_id) REFERENCES folder(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS article (
	id  INTEGER NOT NULL PRIMARY KEY,
	title VARCHAR(255) NOT NULL,
	link VARCHAR(255) NOT NULL,
	published DATETIME NOT NULL DEFAULT(DATETIME('now'))
	-- TODO: content
);

CREATE TABLE IF NOT EXISTS read (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	article_id INTEGER NOT NULL,
	saved BOOLEAN NOT NULL DEFAULT(FALSE),
	UNIQUE(account_id, article_id),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
	FOREIGN KEY(article_id) REFERENCES article(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS filter (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	subscription_id INTEGER,
	name VARCHAR(64),
	find VARCHAR(255),
	is_regex BOOLEAN NOT NULL DEFAULT(FALSE),
	in_title BOOLEAN NOT NULL DEFAULT(TRUE),
	in_content BOOLEAN NOT NULL DEFAULT(FALSE),
	includes BOOLEAN NOT NULL DEFAULT(FALSE),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
	FOREIGN KEY(subscription_id) REFERENCES subscription(id) ON DELETE CASCADE
);

-- TODO: apply filters
