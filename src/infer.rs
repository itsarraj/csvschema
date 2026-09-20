use std::fmt;

/// A SQL column type this tool is willing to infer. Deliberately a small,
/// portable subset (works unquoted on Postgres, MySQL, and SQLite alike) —
/// no attempt at engine-specific types like `SERIAL` or `VARCHAR(n)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Boolean,
    Integer,
    BigInt,
    Double,
    Date,
    Text,
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ColumnType::Boolean => "BOOLEAN",
            ColumnType::Integer => "INTEGER",
            ColumnType::BigInt => "BIGINT",
            ColumnType::Double => "DOUBLE",
            ColumnType::Date => "DATE",
            ColumnType::Text => "TEXT",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    pub name: String,
    pub ty: ColumnType,
    pub nullable: bool,
}

/// Accumulates evidence about one column across every row seen so far. Each
/// `could_be_*` flag starts optimistic (true) and is only ever narrowed —
/// one disqualifying value is permanent, which is what lets a single bad
/// value near the end of a long column correctly widen the whole column to
/// TEXT instead of only the first row being consulted.
#[derive(Debug, Clone)]
struct ColumnAcc {
    saw_any_value: bool,
    nullable: bool,
    could_be_bool: bool,
    could_be_int: bool,
    needs_bigint: bool,
    could_be_double: bool,
    could_be_date: bool,
}

impl ColumnAcc {
    fn new() -> Self {
        ColumnAcc {
            saw_any_value: false,
            nullable: false,
            could_be_bool: true,
            could_be_int: true,
            needs_bigint: false,
            could_be_double: true,
            could_be_date: true,
        }
    }

    fn observe(&mut self, raw: &str) {
        if raw.is_empty() {
            self.nullable = true;
            return;
        }
        self.saw_any_value = true;

        if !is_bool_literal(raw) {
            self.could_be_bool = false;
        }

        if has_leading_zero_padding(raw) {
            // "00123" is numeric-looking but the leading zero means it's an
            // identifier/code (a US zip code, a zero-padded SKU), not a real
            // number — treat it as disqualifying INTEGER/BIGINT/DOUBLE so the
            // column resolves to TEXT and the leading zero survives.
            self.could_be_int = false;
            self.could_be_double = false;
        } else {
            match raw.parse::<i64>() {
                Ok(v) => {
                    if v < i32::MIN as i64 || v > i32::MAX as i64 {
                        self.needs_bigint = true;
                    }
                }
                Err(_) => self.could_be_int = false,
            }
            if raw.parse::<f64>().is_err() {
                self.could_be_double = false;
            }
        }

        if !is_iso_date(raw) {
            self.could_be_date = false;
        }
    }

    fn resolve(&self) -> ColumnType {
        if !self.saw_any_value {
            // Every row had an empty value here — no evidence to infer a
            // real type from, so fall back to the always-safe TEXT rather
            // than guessing.
            return ColumnType::Text;
        }
        if self.could_be_bool {
            ColumnType::Boolean
        } else if self.could_be_int {
            if self.needs_bigint {
                ColumnType::BigInt
            } else {
                ColumnType::Integer
            }
        } else if self.could_be_double {
            ColumnType::Double
        } else if self.could_be_date {
            ColumnType::Date
        } else {
            ColumnType::Text
        }
    }
}

fn is_bool_literal(s: &str) -> bool {
    s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false")
}

fn has_leading_zero_padding(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    digits.len() > 1 && digits.starts_with('0') && digits.chars().all(|c| c.is_ascii_digit())
}

/// A strict `YYYY-MM-DD` check — no other date formats, no time component.
/// Deliberately narrow: widening to accept e.g. `MM/DD/YYYY` too would make
/// this ambiguous with plain division-looking numeric text.
fn is_iso_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return false;
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return false;
    }
    let (Ok(_y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>()) else {
        return false;
    };
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

/// Infers a schema from `headers` plus every row in `rows`, examining at
/// most `sample_limit` rows if given (None means "scan everything").
pub fn infer_schema<I, R>(
    headers: &[String],
    rows: I,
    sample_limit: Option<usize>,
) -> Vec<ColumnSchema>
where
    I: IntoIterator<Item = R>,
    R: AsRef<[String]>,
{
    let mut accs: Vec<ColumnAcc> = headers.iter().map(|_| ColumnAcc::new()).collect();

    for (row_idx, row) in rows.into_iter().enumerate() {
        if let Some(limit) = sample_limit {
            if row_idx >= limit {
                break;
            }
        }
        let row = row.as_ref();
        for (i, acc) in accs.iter_mut().enumerate() {
            let value = row.get(i).map(String::as_str).unwrap_or("");
            acc.observe(value);
        }
    }

    headers
        .iter()
        .zip(accs.iter())
        .map(|(name, acc)| ColumnSchema {
            name: name.clone(),
            ty: acc.resolve(),
            nullable: acc.nullable,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(data: &[&[&str]]) -> Vec<Vec<String>> {
        data.iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn all_integer_column_infers_integer_not_nullable() {
        let h = headers(&["id"]);
        let r = rows(&[&["1"], &["2"], &["3"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Integer);
        assert!(!schema[0].nullable);
    }

    #[test]
    fn a_value_exceeding_i32_widens_to_bigint() {
        let h = headers(&["big"]);
        let r = rows(&[&["1"], &["5000000000"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::BigInt);
    }

    #[test]
    fn mixed_int_and_decimal_infers_double() {
        let h = headers(&["price"]);
        let r = rows(&[&["10"], &["9.99"], &["3.5"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Double);
    }

    #[test]
    fn true_false_strings_infer_boolean() {
        let h = headers(&["active"]);
        let r = rows(&[&["true"], &["false"], &["TRUE"], &["False"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Boolean);
    }

    #[test]
    fn iso_dates_infer_date() {
        let h = headers(&["signup"]);
        let r = rows(&[&["2023-01-15"], &["2024-06-30"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Date);
    }

    #[test]
    fn plain_strings_infer_text() {
        let h = headers(&["name"]);
        let r = rows(&[&["Alice"], &["Bob O'Brien"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Text);
    }

    #[test]
    fn any_empty_value_makes_the_column_nullable() {
        let h = headers(&["middle_name"]);
        let r = rows(&[&["Ray"], &[""], &["Lee"]]);
        let schema = infer_schema(&h, r, None);
        assert!(schema[0].nullable);
        // The empty cell doesn't disqualify the type inferred from the rest.
        assert_eq!(schema[0].ty, ColumnType::Text);
    }

    #[test]
    fn a_column_that_is_empty_in_every_row_falls_back_to_text() {
        let h = headers(&["notes"]);
        let r = rows(&[&[""], &[""]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Text);
        assert!(schema[0].nullable);
    }

    #[test]
    fn zero_padded_numeric_string_infers_text_not_integer() {
        let h = headers(&["zip"]);
        let r = rows(&[&["02134"], &["94107"]]);
        let schema = infer_schema(&h, r, None);
        // 02134 is not integer-parseable under our rule (leading zero), so
        // the whole column — including the non-padded 94107 — falls back
        // to TEXT rather than losing 02134's leading zero as an INTEGER.
        assert_eq!(schema[0].ty, ColumnType::Text);
    }

    // The exact scenario named in the spec: 999 clean integers plus one
    // non-numeric value must widen the whole column to TEXT — proving the
    // inference actually scans every row, not just the first one.
    #[test]
    fn a_single_bad_value_late_in_a_long_column_widens_the_whole_column_to_text() {
        let h = headers(&["mostly_int"]);
        let mut data: Vec<Vec<String>> = (0..999).map(|i| vec![i.to_string()]).collect();
        data.push(vec!["N/A".to_string()]);
        let schema = infer_schema(&h, data, None);
        assert_eq!(schema[0].ty, ColumnType::Text);
    }

    #[test]
    fn sample_limit_only_examines_the_first_n_rows() {
        let h = headers(&["mostly_int"]);
        let mut data: Vec<Vec<String>> = (0..999).map(|i| vec![i.to_string()]).collect();
        data.push(vec!["N/A".to_string()]); // row 999, past a 500-row sample
        let schema = infer_schema(&h, data, Some(500));
        // With sampling capped before the bad row, it looks like a clean
        // integer column — this documents the real tradeoff of --sample,
        // not a bug.
        assert_eq!(schema[0].ty, ColumnType::Integer);
    }

    #[test]
    fn missing_trailing_field_is_treated_as_an_empty_nullable_value() {
        let h = headers(&["a", "b"]);
        let r = vec![
            vec!["1".to_string()],
            vec!["2".to_string(), "3".to_string()],
        ];
        let schema = infer_schema(&h, r, None);
        assert!(schema[1].nullable);
        assert_eq!(schema[1].ty, ColumnType::Integer);
    }

    #[test]
    fn negative_integers_are_recognized() {
        let h = headers(&["delta"]);
        let r = rows(&[&["-5"], &["10"], &["-2147483649"]]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::BigInt);
    }

    #[test]
    fn a_realistic_mixed_schema_infers_every_column_correctly() {
        let h = headers(&[
            "id",
            "name",
            "email",
            "age",
            "active",
            "signup_date",
            "middle_name",
        ]);
        let r = rows(&[
            &[
                "1",
                "Alice",
                "alice@example.com",
                "30",
                "true",
                "2023-01-15",
                "Jane",
            ],
            &[
                "2",
                "Bob",
                "bob@example.com",
                "25",
                "false",
                "2023-02-20",
                "",
            ],
            &[
                "3",
                "Carol",
                "carol@example.com",
                "40",
                "true",
                "2023-03-05",
                "Ann",
            ],
        ]);
        let schema = infer_schema(&h, r, None);
        assert_eq!(schema[0].ty, ColumnType::Integer); // id
        assert_eq!(schema[1].ty, ColumnType::Text); // name
        assert_eq!(schema[2].ty, ColumnType::Text); // email
        assert_eq!(schema[3].ty, ColumnType::Integer); // age
        assert_eq!(schema[4].ty, ColumnType::Boolean); // active
        assert_eq!(schema[5].ty, ColumnType::Date); // signup_date
        assert_eq!(schema[6].ty, ColumnType::Text); // middle_name
        assert!(schema[6].nullable);
        assert!(!schema[0].nullable);
    }
}
