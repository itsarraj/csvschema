use std::fs::File;
use std::path::{Path, PathBuf};

use clap::Parser;

use csvschema::infer::infer_schema;
use csvschema::sql::render_create_table;

#[derive(Parser)]
#[command(
    name = "csvschema",
    about = "Infers a SQL CREATE TABLE statement from a real CSV file's actual data"
)]
struct Cli {
    /// Path to the CSV file (must have a header row)
    csv_path: PathBuf,

    /// Table name to use (defaults to the file's stem, e.g. users.csv -> users)
    #[arg(long)]
    table: Option<String>,

    /// Only examine the first N data rows (default: scan every row)
    #[arg(long)]
    sample: Option<usize>,
}

fn default_table_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data")
        .to_string()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let table = cli
        .table
        .unwrap_or_else(|| default_table_name(&cli.csv_path));

    let file = File::open(&cli.csv_path)
        .map_err(|e| anyhow::anyhow!("opening {}: {e}", cli.csv_path.display()))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| anyhow::anyhow!("reading header row: {e}"))?
        .iter()
        .map(str::to_string)
        .collect();

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| anyhow::anyhow!("reading a CSV row: {e}"))?;
        rows.push(record.iter().map(str::to_string).collect::<Vec<String>>());
    }

    let schema = infer_schema(&headers, rows, cli.sample);
    println!("{}", render_create_table(&table, &schema));

    Ok(())
}
