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

/// Return all lat/lon coordinate pairs that pass the active filters and optional bounding box.
/// When `min_lat`/`max_lat`/`min_lon`/`max_lon` are all Some, only rows within that bbox
/// are returned. When all are None the full (filtered) dataset is returned so the frontend
/// can compute a fit-bounds extent on first load.
#[tauri::command]
fn get_map_points(
    lat_col: String,
    lon_col: String,
    filters: Vec<datastore::FilterSpec>,
    min_lat: Option<f64>,
    max_lat: Option<f64>,
    min_lon: Option<f64>,
    max_lon: Option<f64>,
    state: State<'_, AppState>,
) -> Result<Vec<[f64; 2]>, String> {
    let (file_path, schema) = {
        let guard = state.file.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No file loaded")?;
        (loaded.path.clone(), loaded.schema.clone())
    };

    let mut lf = datastore::scan_file(&file_path)?;
    lf = datastore::apply_filters(lf, &filters, &schema)?;

    // Apply bounding-box filter when all four bounds are provided.
    if let (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) =
        (min_lat, max_lat, min_lon, max_lon)
    {
        lf = lf.filter(
            col(lat_col.as_str())
                .gt_eq(lit(min_lat))
                .and(col(lat_col.as_str()).lt_eq(lit(max_lat)))
                .and(col(lon_col.as_str()).gt_eq(lit(min_lon)))
                .and(col(lon_col.as_str()).lt_eq(lit(max_lon))),
        );
    }

    let df = lf
        .select([col(lat_col.as_str()), col(lon_col.as_str())])
        .collect()
        .map_err(|e| e.to_string())?;

    let lat_series = df
        .column(&lat_col)
        .map_err(|e| e.to_string())?
        .cast(&DataType::Float64)
        .map_err(|e| e.to_string())?;
    let lon_series = df
        .column(&lon_col)
        .map_err(|e| e.to_string())?
        .cast(&DataType::Float64)
        .map_err(|e| e.to_string())?;

    let lats = lat_series.f64().map_err(|e| e.to_string())?;
    let lons = lon_series.f64().map_err(|e| e.to_string())?;

    let points: Vec<[f64; 2]> = lats
        .iter()
        .zip(lons.iter())
        .filter_map(|(lat, lon)| match (lat, lon) {
            (Some(lat), Some(lon)) => Some([lat, lon]),
            _ => None,
        })
        .collect();

    Ok(points)
}

/// Return all H3 cell index values that pass the active filters as strings.
/// The frontend decodes each index to a polygon boundary using h3-js.
#[tauri::command]
fn get_h3_values(
    h3_col: String,
    filters: Vec<datastore::FilterSpec>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let (file_path, schema) = {
        let guard = state.file.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No file loaded")?;
        (loaded.path.clone(), loaded.schema.clone())
    };

    let mut lf = datastore::scan_file(&file_path)?;
    lf = datastore::apply_filters(lf, &filters, &schema)?;

    let df = lf
        .select([col(h3_col.as_str())])
        .collect()
        .map_err(|e| e.to_string())?;

    let series = df
        .column(&h3_col)
        .map_err(|e| e.to_string())?
        .cast(&DataType::String)
        .map_err(|e| e.to_string())?;

    let out: Vec<String> = series
        .str()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|v| v.map(|s| s.to_string()))
        .collect();

    Ok(out)
}

#[tauri::command]
fn get_chart_data(
    x_col: String,
    y_cols: Vec<String>,
    filters: Vec<datastore::FilterSpec>,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let (file_path, schema) = {
        let guard = state.file.lock().unwrap();
        let loaded = guard.as_ref().ok_or("No file loaded")?;
        (loaded.path.clone(), loaded.schema.clone())
    };

    datastore::get_chart_rows(&file_path, &x_col, &y_cols, &filters, &schema)
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
        .invoke_handler(tauri::generate_handler![load_file, get_rows, export_file, get_map_points, get_h3_values, get_chart_data])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
