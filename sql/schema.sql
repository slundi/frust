CREATE TABLE account (
	id  INTEGER NOT NULL PRIMARY KEY,
	slug VARCHAR(36) NOT NULL,
	email       VARCHAR(200) NOT NULL,
	username  VARCHAR(32) NOT NULL UNIQUE,
	encrypted_password   VARCHAR(72) NOT NULL,
	config    TEXT NOT NULL DEFAULT '{}',
	UNIQUE(slug)
);

CREATE TABLE token (
	id  INTEGER NOT NULL PRIMARY KEY,
	slug VARCHAR(36) NOT NULL,
	account_id INTEGER NOT NULL,
	created DATETIME NOT NULL DEFAULT NOW(),
	name VARCHAR(64),
	UNIQUE(slug),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE folder (
	id  INTEGER NOT NULL PRIMARY KEY,
	slug VARCHAR(36) NOT NULL UNIQUE,
	account_id INTEGER NOT NULL,
	name VARCHAR(64),
	UNIQUE(slug),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE feed (
	id  INTEGER NOT NULL PRIMARY KEY,
	slug VARCHAR(36) NOT NULL,
	title VARCHAR(255) NOT NULL,
	link VARCHAR(255) NOT NULL,
	description TEXT NOT NULL,	
	language TEXT,
	copyright TEXT,
	managing_editor VARCHAR(255),
	webmaster VARCHAR(255),
	publication_date DATETIME,
	last_build_date DATETIME,
	category VARCHAR(64),
	generator VARCHAR(64),
	ttl INTEGER,
	image TEXT,
	extra TEXT,
	-- TODO: improve, ATOM support
	UNIQUE(slug),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE subscription (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	feed_id INTEGER NOT NULL,
	folder_id INTEGER NOT NULL,
	xpath VARCHAR(255),
	UNIQUE(account_id, feed_id),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
	FOREIGN KEY(feed_id) REFERENCES feed(id) ON DELETE CASCADE
);

CREATE TABLE article (
	id  INTEGER NOT NULL PRIMARY KEY,
	slug VARCHAR(36) NOT NULL,
	title VARCHAR(255) NOT NULL,
	link VARCHAR(255) NOT NULL,
	-- TODO
	UNIQUE(slug),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

CREATE TABLE subscription (
	id  INTEGER NOT NULL PRIMARY KEY,
	account_id INTEGER NOT NULL,
	article_id INTEGER NOT NULL,
	saved BOOLEAN NOT NULL DEFAULT(FALSE),
	UNIQUE(account_id, feed_id),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
	FOREIGN KEY(article_id) REFERENCES article(id) ON DELETE CASCADE
);

CREATE TABLE filter (
	id  INTEGER NOT NULL PRIMARY KEY,
	slug VARCHAR(36) NOT NULL UNIQUE,
	account_id INTEGER NOT NULL,
	name VARCHAR(64),
	find VARCHAR(255),
	is_regex BOOLEAN NOT NULL DEFAULT(FALSE)
	name VARCHAR(64),
	UNIQUE(slug),
	FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE
);

-- TODO: apply filters
