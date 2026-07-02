use driver_api::SqlDialect;

pub struct PostgreDialect;

impl SqlDialect for PostgreDialect {
    fn quote_identifier(&self, ident: &str) -> String {
        if ident.contains('.') {
            ident
                .split('.')
                .map(|part| format!("\"{}\"", part.replace("\"", "\"\"")))
                .collect::<Vec<String>>()
                .join(".")
        } else {
            format!("\"{}\"", ident.replace("\"", "\"\""))
        }
    }

    fn escape_string_literal(&self, val: &str) -> String {
        val.replace("'", "''")
    }

    fn get_type_cast_clause(&self, column_type: &str) -> Option<String> {
        let ty = column_type.to_lowercase();
        if ty.contains("json") || ty.contains("xml") || ty.contains("enum") {
            Some("::text".to_string())
        } else {
            None
        }
    }

    fn transform_query_limit(&self, sql: &str, limit: usize, offset: Option<usize>) -> String {
        let trimmed = sql.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("select") || lower.starts_with("with") {
            if let Some(off) = offset {
                format!(
                    "SELECT * FROM ({}) AS _odv_wrapper LIMIT {} OFFSET {}",
                    trimmed, limit, off
                )
            } else {
                format!("SELECT * FROM ({}) AS _odv_wrapper LIMIT {}", trimmed, limit)
            }
        } else {
            sql.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_identifier() {
        let dialect = PostgreDialect;
        assert_eq!(dialect.quote_identifier("users"), "\"users\"");
        assert_eq!(dialect.quote_identifier("public.users"), "\"public\".\"users\"");
        assert_eq!(
            dialect.quote_identifier("my\"table"),
            "\"my\"\"table\""
        );
    }

    #[test]
    fn test_escape_string_literal() {
        let dialect = PostgreDialect;
        assert_eq!(dialect.escape_string_literal("hello"), "hello");
        assert_eq!(dialect.escape_string_literal("O'Connor"), "O''Connor");
    }

    #[test]
    fn test_transform_query_limit() {
        let dialect = PostgreDialect;
        assert_eq!(
            dialect.transform_query_limit("SELECT * FROM users", 10, None),
            "SELECT * FROM (SELECT * FROM users) AS _odv_wrapper LIMIT 10"
        );
        assert_eq!(
            dialect.transform_query_limit("SELECT * FROM users", 10, Some(5)),
            "SELECT * FROM (SELECT * FROM users) AS _odv_wrapper LIMIT 10 OFFSET 5"
        );
        assert_eq!(
            dialect.transform_query_limit("INSERT INTO users VALUES (1)", 10, None),
            "INSERT INTO users VALUES (1)"
        );
    }
}
