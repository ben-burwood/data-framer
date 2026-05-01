<script setup lang="ts">
import { ref, computed } from "vue";
import { AgGridVue } from "ag-grid-vue3";
import { ModuleRegistry, AllCommunityModule } from "ag-grid-community";
import type {
  ColDef,
  GridApi,
  GridReadyEvent,
  IDatasource,
  IGetRowsParams,
} from "ag-grid-community";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import "ag-grid-community/styles/ag-grid.css";
import "ag-grid-community/styles/ag-theme-quartz.css";

ModuleRegistry.registerModules([AllCommunityModule]);

// ---------------------------------------------------------------------------
// Types mirroring the Rust structs
// ---------------------------------------------------------------------------
type Dtype = "integer" | "float" | "boolean" | "date" | "datetime" | "string";

interface ColumnInfo {
  name: string;
  dtype: Dtype;
}

interface FileInfo {
  path: string;
  total_rows: number;
  columns: ColumnInfo[];
}

interface RowsResponse {
  rows: Record<string, unknown>[];
  total_rows: number;
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
type ViewState = "empty" | "loading" | "loaded";

const view = ref<ViewState>("empty");
const fileInfo = ref<FileInfo | null>(null);
const gridApi = ref<GridApi | null>(null);

const columnDefs = computed<ColDef[]>(() =>
  fileInfo.value ? schemaToColDefs(fileInfo.value.columns) : []
);
const fileName = computed(() => fileInfo.value?.path.split(/[\\/]/).pop() ?? "");

const defaultColDef: ColDef = {
  flex: 1,
  minWidth: 80,
  sortable: true,
  resizable: true,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function schemaToColDefs(columns: ColumnInfo[]): ColDef[] {
  return columns.map((c) => ({
    field: c.name,
    headerName: c.name,
    ...(c.dtype === "integer" || c.dtype === "float"
      ? { type: "numericColumn" }
      : {}),
  }));
}

function buildDatasource(): IDatasource {
  return {
    getRows(params: IGetRowsParams) {
      const { startRow, endRow, sortModel } = params;
      invoke<RowsResponse>("get_rows", {
        offset: startRow,
        limit: endRow - startRow,
        sortCol: sortModel[0]?.colId ?? null,
        sortDesc: sortModel[0]?.sort === "desc",
      })
        .then((r) => params.successCallback(r.rows, r.total_rows))
        .catch((err) => {
          console.error("get_rows failed:", err);
          params.failCallback();
        });
    },
  };
}

// ---------------------------------------------------------------------------
// Grid events
// ---------------------------------------------------------------------------
function onGridReady(event: GridReadyEvent) {
  gridApi.value = event.api;
  // If a file is already loaded (e.g. after hot-reload), wire up the datasource.
  if (fileInfo.value) {
    event.api.setGridOption("datasource", buildDatasource());
  }
}

// ---------------------------------------------------------------------------
// File open
// ---------------------------------------------------------------------------
async function openFile() {
  const path = (await open({
    multiple: false,
    filters: [{ name: "Data Files", extensions: ["parquet", "csv"] }],
  })) as string | null;

  if (!path) return;

  view.value = "loading";
  gridApi.value = null;

  try {
    const info = await invoke<FileInfo>("load_file", { path });
    fileInfo.value = info;
    view.value = "loaded";
    // onGridReady fires after the grid mounts and sets the datasource there.
  } catch (err) {
    view.value = "empty";
    alert(`Failed to load file:\n${err}`);
  }
}
</script>

<template>
  <div class="app">
    <!-- Empty / loading overlay -->
    <div v-if="view !== 'loaded'" class="overlay">
      <div class="drop-zone">
        <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round"
            d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z" />
        </svg>
        <p class="hint">Open a Parquet or CSV file to get started</p>
        <button @click="openFile" :disabled="view === 'loading'">
          {{ view === "loading" ? "Loading…" : "Open File" }}
        </button>
      </div>
    </div>

    <!-- Loaded state -->
    <template v-else>
      <div class="toolbar">
        <span class="file-name">{{ fileName }}</span>
        <span class="row-count">{{ fileInfo!.total_rows.toLocaleString() }} rows</span>
        <button class="toolbar-btn" @click="openFile">Open File</button>
      </div>
      <AgGridVue
        class="ag-theme-quartz grid"
        :columnDefs="columnDefs"
        :defaultColDef="defaultColDef"
        rowModelType="infinite"
        :cacheBlockSize="200"
        :maxBlocksInCache="20"
        :infiniteInitialRowCount="1"
        @grid-ready="onGridReady"
      />
    </template>
  </div>
</template>

<style>
*,
*::before,
*::after {
  box-sizing: border-box;
}

html,
body,
#app {
  height: 100%;
  margin: 0;
  padding: 0;
  overflow: hidden;
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
}
</style>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: #fff;
}

/* ---- Empty / loading ---- */
.overlay {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.drop-zone {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 48px 64px;
  border: 2px dashed #d0d0d0;
  border-radius: 16px;
  color: #555;
}

.icon {
  width: 48px;
  height: 48px;
  color: #aaa;
}

.hint {
  margin: 0;
  font-size: 0.95rem;
  color: #888;
}

button {
  padding: 10px 28px;
  font-size: 0.95rem;
  font-family: inherit;
  border-radius: 8px;
  border: none;
  background: #646cff;
  color: #fff;
  cursor: pointer;
  transition: background 0.15s;
}

button:hover:not(:disabled) {
  background: #535bf2;
}

button:disabled {
  opacity: 0.55;
  cursor: default;
}

/* ---- Toolbar ---- */
.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 14px;
  background: #f5f5f5;
  border-bottom: 1px solid #e0e0e0;
  font-size: 0.85rem;
  flex-shrink: 0;
}

.file-name {
  font-weight: 600;
  color: #222;
}

.row-count {
  color: #777;
}

.toolbar-btn {
  margin-left: auto;
  padding: 4px 14px;
  font-size: 0.8rem;
}

/* ---- Grid ---- */
.grid {
  flex: 1;
  min-height: 0;
}
</style>
