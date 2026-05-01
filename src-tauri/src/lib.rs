mod datastore;

use datastore::{AppState, FileInfo, LoadedFile, RowsResponse};
use polars::prelude::*;
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

/// Return a paginated, optionally sorted slice of the loaded file.
/// Only `limit` rows are collected and sent over IPC.
#[tauri::command]
fn get_rows(
    offset: i64,
    limit: i64,
    sort_col: Option<String>,
    sort_desc: bool,
    state: State<'_, AppState>,
) -> Result<RowsResponse, String> {
    let (file_path, total_rows) = {
        let guard = state.file.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No file loaded")?;
        (loaded.path.clone(), loaded.total_rows)
    };

    let mut lf = datastore::scan_file(&file_path)?;

    if let Some(col) = sort_col {
        lf = lf.sort(
            [col.as_str()],
            SortMultipleOptions::default().with_order_descending(sort_desc),
        );
    }

    let df = lf
        .slice(offset, limit as u32)
        .collect()
        .map_err(|e| e.to_string())?;

    let rows = datastore::frame_to_rows(&df);

    Ok(RowsResponse { rows, total_rows })
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
        .invoke_handler(tauri::generate_handler![load_file, get_rows])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
