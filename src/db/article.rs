const SQL_DELETE_ALL_USER_ARTICLE: &str = "DELETE FROM read WHERE account_id = $1";
/// Remove unsaved articles that are older than `Config.article_keep_time` (in days)
const SQL_REMOVE_OLD_ARTICLES: &str ="DELETE FROM article WHERE id NOT IN (SELECT article_id FROM read) AND published > ($1+TODO)";
