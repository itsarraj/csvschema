# csvschema

Infers a SQL `CREATE TABLE` statement from a real CSV file's actual
data — column types and nullability from every row, not a guess from the
first one. The "load this CSV into Postgres" first step everyone
hand-writes a throwaway script for.

## Usage

```bash
csvschema users.csv                 # table name defaults to the file stem ("users")
csvschema users.csv --table people  # override the table name
csvschema users.csv --sample 1000   # only examine the first 1000 rows (default: every row)
```

## How type inference works

Every column is checked against every sampled row, not just the first:
a column that's numeric-looking in 999 rows but has one non-numeric
value correctly widens to `TEXT` rather than staying `INTEGER` and
silently failing to insert that one real row later. Type ranking (most
to least specific): `BOOLEAN` (`true`/`false`, case-insensitive) →
`INTEGER` → `BIGINT` (a number too large for `INTEGER`) → `DOUBLE
PRECISION` (has a decimal point) → `DATE` (`YYYY-MM-DD`) → `TEXT` (the
fallback for anything else, and for a column with genuinely mixed
types). Any empty value anywhere in a column makes that column
nullable; a column with no empty values gets `NOT NULL`.

## Status: built and verified against a realistic multi-type CSV file

- **20 unit tests** (`cargo test --lib`) across `infer` (type widening —
  a mostly-integer column with one text value widening to `TEXT`, a
  boolean-looking column, a date column, an empty value anywhere marking
  a column nullable, an all-empty column) and `sql` (rendering a correct
  `CREATE TABLE` statement, including identifier quoting for a column
  name that isn't a plain identifier).
- **Live-verified against the actual compiled binary and a realistic
  5-column CSV file** (an integer ID, a text name, a boolean-like
  column, a `YYYY-MM-DD` date column, and a notes column with some empty
  values): the generated `CREATE TABLE` correctly typed all five columns
  and correctly marked only the notes column nullable — the one column
  that actually had empty values in the sample.

**Not done / deliberately deferred**: dialect-specific type names (this
emits ANSI-ish types — `INTEGER`, `BIGINT`, `DOUBLE PRECISION`, `DATE`,
`TEXT`, `BOOLEAN` — that Postgres accepts directly; MySQL/SQLite users
will want to adjust a few, e.g. `DOUBLE PRECISION` → `DOUBLE`); primary
key / unique constraint inference (an all-unique integer column that's
obviously an ID isn't automatically marked `PRIMARY KEY` — this only
infers column types, not constraints); and timestamp-with-time
inference (only bare `YYYY-MM-DD` dates are recognized as `DATE` — a
datetime column falls through to `TEXT`).
