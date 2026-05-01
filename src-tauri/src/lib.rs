mod datastore;

use datastore::{AppState, FileInfo, LoadedFile, RowsResponse};
use std::sync::Mutex;
use tauri::State;

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Load a parquet or CSV file: reads schema + row count (no data), stores
/// the file path in managed state, and returns metadata to the frontend.
#[tauri::command]
fn load_file(path: String, state: State<'_, AppState>) -> Result<FileInfo, String> {
    let mut lf = datastore::scan_file(&path)?;
    let columns = datastore::extract_schema(&mut lf)?;
    let total_rows = datastore::count_rows(lf, &path)?;

    *state.file.lock().unwrap() = Some(LoadedFile {
        path: path.clone(),
        total_rows,
        schema: columns.clone(),
    });

    Ok(FileInfo {
        path,
        total_rows,
        columns,
    })
}

/// Return a paginated, optionally sorted, optionally filtered, optionally column-projected
/// slice of the loaded file. Only `limit` rows are collected and sent over IPC.
#[tauri::command]
fn get_rows(
    offset: i64,
    limit: i64,
    sort_col: Option<String>,
    sort_desc: bool,
    filters: Vec<datastore::FilterSpec>,
    columns: Vec<String>,
    state: State<'_, AppState>,
) -> Result<RowsResponse, String> {
    let (file_path, unfiltered_rows, schema) = {
        let guard = state.file.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No file loaded")?;
        (loaded.path.clone(), loaded.total_rows, loaded.schema.clone())
    };

    let lf = datastore::build_pipeline(
        &file_path,
        &filters,
        &schema,
        sort_col.as_deref(),
        sort_desc,
        &columns,
    )?;

    // Count filtered rows only when filters are active (avoids a full scan otherwise).
    // Sort and column projection don't affect row count, so counting on the full pipeline is fine.
    let total_rows = if filters.is_empty() {
        unfiltered_rows
    } else {
        datastore::count_lf(&lf)?
    };

    let df = lf
        .slice(offset, limit as u32)
        .collect()
        .map_err(|e| e.to_string())?;

    Ok(RowsResponse { rows: datastore::frame_to_rows(&df), total_rows })
}

/// Export the current view (with active sort, filters, and column selection) to a file.
/// Format is inferred from `dest`'s extension: `.parquet` → Parquet, else CSV.
#[tauri::command]
fn export_file(
    dest: String,
    sort_col: Option<String>,
    sort_desc: bool,
    filters: Vec<datastore::FilterSpec>,
    columns: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (file_path, schema) = {
        let guard = state.file.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No file loaded")?;
        (loaded.path.clone(), loaded.schema.clone())
    };

    let lf = datastore::build_pipeline(
        &file_path,
        &filters,
        &schema,
        sort_col.as_deref(),
        sort_desc,
        &columns,
    )?;
    let mut df = lf.collect().map_err(|e| e.to_string())?;
    datastore::write_file(&mut df, &dest)
}

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            file: Mutex::new(None),
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![load_file, get_rows, export_file])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
