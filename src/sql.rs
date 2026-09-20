use crate::infer::ColumnSchema;

/// Quotes an identifier with double quotes if it isn't already a plain,
/// unquoted-safe SQL identifier (ASCII letters/digits/underscore, not
/// starting with a digit) — most CSV headers don't need this, but a header
/// like `"first name"` or `"order#"` does.
fn quote_identifier(name: &str) -> String {
    let is_plain = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.chars().next().unwrap().is_ascii_digit();
    if is_plain {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

pub fn render_create_table(table: &str, columns: &[ColumnSchema]) -> String {
    let body: Vec<String> = columns
        .iter()
        .map(|col| {
            let name = quote_identifier(&col.name);
            let null_clause = if col.nullable { "" } else { " NOT NULL" };
            format!("    {name} {}{null_clause}", col.ty)
        })
        .collect();
    format!("CREATE TABLE {table} (\n{}\n);", body.join(",\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infer::ColumnType;

    fn col(name: &str, ty: ColumnType, nullable: bool) -> ColumnSchema {
        ColumnSchema {
            name: name.to_string(),
            ty,
            nullable,
        }
    }

    #[test]
    fn renders_not_null_for_non_nullable_columns() {
        let sql = render_create_table("users", &[col("id", ColumnType::Integer, false)]);
        assert!(sql.contains("id INTEGER NOT NULL"));
    }

    #[test]
    fn omits_not_null_for_nullable_columns() {
        let sql = render_create_table("users", &[col("middle_name", ColumnType::Text, true)]);
        assert!(sql.contains("middle_name TEXT"));
        assert!(!sql.contains("middle_name TEXT NOT NULL"));
    }

    #[test]
    fn joins_multiple_columns_with_trailing_commas_except_the_last() {
        let sql = render_create_table(
            "t",
            &[
                col("id", ColumnType::Integer, false),
                col("name", ColumnType::Text, true),
            ],
        );
        assert_eq!(
            sql,
            "CREATE TABLE t (\n    id INTEGER NOT NULL,\n    name TEXT\n);"
        );
    }

    #[test]
    fn quotes_an_identifier_with_a_space() {
        let sql = render_create_table("t", &[col("first name", ColumnType::Text, true)]);
        assert!(sql.contains("\"first name\" TEXT"));
    }

    #[test]
    fn quotes_an_identifier_starting_with_a_digit() {
        let sql = render_create_table("t", &[col("2fa_enabled", ColumnType::Boolean, false)]);
        assert!(sql.contains("\"2fa_enabled\" BOOLEAN"));
    }

    #[test]
    fn plain_identifiers_are_left_unquoted() {
        let sql = render_create_table("t", &[col("email", ColumnType::Text, false)]);
        assert!(sql.contains("    email TEXT NOT NULL"));
        assert!(!sql.contains("\"email\""));
    }
}
