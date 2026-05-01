use polars::prelude::*;
use serde::Serialize;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub dtype: String,
}

/// Persisted across `get_rows` calls so we don't re-read the file header each time.
#[derive(Debug)]
pub struct LoadedFile {
    pub path: String,
    pub total_rows: usize,
    pub schema: Vec<ColumnInfo>,
}

/// Tauri managed state — wrapped in Mutex so commands can mutate it.
pub struct AppState {
    pub file: Mutex<Option<LoadedFile>>,
}

/// Returned by `load_file` command.
#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub total_rows: usize,
    pub columns: Vec<ColumnInfo>,
}

/// Returned by `get_rows` command.
#[derive(Debug, Serialize)]
pub struct RowsResponse {
    pub rows: Vec<serde_json::Value>,
    pub total_rows: usize,
}

// ---------------------------------------------------------------------------
// File scanning
// ---------------------------------------------------------------------------

/// Build a LazyFrame from a file path. Supports .parquet and .csv.
pub fn scan_file(path: &str) -> Result<LazyFrame, String> {
    match file_ext(path).as_str() {
        "parquet" => {
            LazyFrame::scan_parquet(path.into(), ScanArgsParquet::default())
                .map_err(|e| e.to_string())
        }
        "csv" => {
            LazyCsvReader::new(path.into()).finish().map_err(|e| e.to_string())
        }
        other => Err(format!("Unsupported file format: .{other}")),
    }
}

/// Row count from an already-scanned LazyFrame, avoiding a second file open.
///   Parquet: `len()` reads row-group footer metadata only (no row data loaded).
///   CSV: counts newlines minus the header row (streaming, no data heap alloc).
pub fn count_rows(lf: LazyFrame, path: &str) -> Result<usize, String> {
    match file_ext(path).as_str() {
        "parquet" => {
            let df = lf
                .select([len().alias("_n")])
                .collect()
                .map_err(|e| e.to_string())?;
            df.column("_n")
                .map_err(|e| e.to_string())?
                .get(0)
                .map_err(|e| e.to_string())?
                .try_extract::<usize>()
                .map_err(|e| e.to_string())
        }
        _ => {
            use std::io::BufRead;
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            Ok(std::io::BufReader::new(file)
                .lines()
                .count()
                .saturating_sub(1))
        }
    }
}

// ---------------------------------------------------------------------------
// Schema helpers
// ---------------------------------------------------------------------------

/// Extract column metadata from a LazyFrame without collecting any data.
pub fn extract_schema(lf: &mut LazyFrame) -> Result<Vec<ColumnInfo>, String> {
    // collect_schema() reads file metadata only (no rows loaded).
    let schema = lf.collect_schema().map_err(|e| e.to_string())?;
    Ok(schema
        .iter()
        .map(|(name, dtype)| ColumnInfo {
            name: name.to_string(),
            dtype: dtype_to_str(dtype).to_string(),
        })
        .collect())
}

/// Map a Polars DataType to a simple string the frontend can use for column typing.
pub fn dtype_to_str(dtype: &DataType) -> &'static str {
    match dtype {
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => "integer",
        DataType::Float32 | DataType::Float64 => "float",
        DataType::Boolean => "boolean",
        DataType::Date => "date",
        DataType::Datetime(_, _) => "datetime",
        _ => "string",
    }
}

// ---------------------------------------------------------------------------
// DataFrame → JSON rows
// ---------------------------------------------------------------------------

/// Convert a (small, paginated) DataFrame into a Vec of JSON objects
/// suitable for serialisation over Tauri IPC.
pub fn frame_to_rows(df: &DataFrame) -> Vec<serde_json::Value> {
    let columns = df.columns();
    // Pre-extract column names once so the inner loop doesn't allocate per cell.
    let names: Vec<String> = columns.iter().map(|c| c.name().to_string()).collect();
    let height = df.height();
    let mut rows = Vec::with_capacity(height);
    for i in 0..height {
        let mut map = serde_json::Map::with_capacity(columns.len());
        for (col, name) in columns.iter().zip(&names) {
            let val: serde_json::Value = col
                .get(i)
                .map(anyvalue_to_json)
                .unwrap_or(serde_json::Value::Null);
            map.insert(name.clone(), val);
        }
        rows.push(serde_json::Value::Object(map));
    }
    rows
}

fn anyvalue_to_json(v: AnyValue<'_>) -> serde_json::Value {
    match v {
        AnyValue::Null => serde_json::Value::Null,
        AnyValue::Boolean(b) => serde_json::Value::Bool(b),
        AnyValue::Int8(n) => n.into(),
        AnyValue::Int16(n) => n.into(),
        AnyValue::Int32(n) => n.into(),
        AnyValue::Int64(n) => n.into(),
        AnyValue::UInt8(n) => n.into(),
        AnyValue::UInt16(n) => n.into(),
        AnyValue::UInt32(n) => n.into(),
        AnyValue::UInt64(n) => n.into(),
        AnyValue::Float32(f) => serde_json::Number::from_f64(f as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        AnyValue::Float64(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        AnyValue::String(s) => serde_json::Value::String(s.to_string()),
        AnyValue::StringOwned(s) => serde_json::Value::String(s.to_string()),
        // Dates, datetimes, categoricals — fall back to Display impl
        other => serde_json::Value::String(format!("{other}")),
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn file_ext(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}
