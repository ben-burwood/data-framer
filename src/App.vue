<script setup lang="ts">
import { ref, computed } from "vue";
import type { ColumnState, GridApi } from "ag-grid-community";
import { open, save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import type { FileInfo, FilterSpec } from "./types";
import FilterPanel from "./components/FilterPanel.vue";
import SelectPanel from "./components/SelectPanel.vue";
import DataGrid from "./components/DataGrid.vue";
import MapView from "./components/MapView.vue";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
type ViewState = "empty" | "loading" | "loaded";

const view = ref<ViewState>("empty");
const fileInfo = ref<FileInfo | null>(null);
const activeFilters = ref<FilterSpec[]>([]);
const activeColumnVisibility = ref<Record<string, boolean>>({});
const currentView = ref<"table" | "map">("table");
const exportState = ref<"idle" | "exporting">("idle");
const filteredRowCount = ref(0);
const gridApi = ref<GridApi | null>(null);
const filterPanelOpen = ref(false);
const columnPanelOpen = ref(false);

// ---------------------------------------------------------------------------
// Computed
// ---------------------------------------------------------------------------
const LAT_NAMES = ["lat", "latitude"];
const LON_NAMES = ["lon", "lng", "longitude"];

const fileName = computed(() => fileInfo.value?.path.split(/[\\/]/).pop() ?? "");

const latColumn = computed(() =>
  fileInfo.value?.columns.find(c => LAT_NAMES.includes(c.name.toLowerCase()))?.name ?? null
);
const lonColumn = computed(() =>
  fileInfo.value?.columns.find(c => LON_NAMES.includes(c.name.toLowerCase()))?.name ?? null
);
const hasGeoColumns = computed(() => !!latColumn.value && !!lonColumn.value);

const hiddenColumnCount = computed(() => {
  if (!fileInfo.value) return 0;
  return fileInfo.value.columns.filter(c => activeColumnVisibility.value[c.name] === false).length;
});

// ---------------------------------------------------------------------------
// File open
// ---------------------------------------------------------------------------
function initColumnVisibility() {
  const vis: Record<string, boolean> = {};
  for (const c of fileInfo.value!.columns) vis[c.name] = true;
  activeColumnVisibility.value = vis;
}

async function openFile() {
  const path = (await open({
    multiple: false,
    filters: [{ name: "Data Files", extensions: ["parquet", "csv"] }],
  })) as string | null;
  if (!path) return;

  view.value = "loading";
  gridApi.value = null;
  filterPanelOpen.value = false;
  activeFilters.value = [];
  filteredRowCount.value = 0;
  columnPanelOpen.value = false;

  try {
    const info = await invoke<FileInfo>("load_file", { path });
    fileInfo.value = info;
    initColumnVisibility();
    currentView.value = "table";
    view.value = "loaded";
  } catch (err) {
    view.value = "empty";
    alert(`Failed to load file:\n${err}`);
  }
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------
async function exportFile() {
  const dest = (await save({
    filters: [
      { name: "CSV",     extensions: ["csv"]     },
      { name: "Parquet", extensions: ["parquet"] },
    ],
  })) as string | null;
  if (!dest) return;

  const sortedCol = (gridApi.value?.getColumnState() as ColumnState[] ?? [])
    .find(c => c.sort != null);

  const allCols = fileInfo.value!.columns;
  const visible = allCols.filter(c => activeColumnVisibility.value[c.name] !== false);
  const columns = visible.length < allCols.length ? visible.map(c => c.name) : [];

  exportState.value = "exporting";
  try {
    await invoke("export_file", {
      dest,
      sortCol:  sortedCol?.colId  ?? null,
      sortDesc: sortedCol?.sort === "desc",
      filters:  activeFilters.value,
      columns,
    });
  } catch (err) {
    alert(`Export failed:\n${err}`);
  } finally {
    exportState.value = "idle";
  }
}

// ---------------------------------------------------------------------------
// Filter / column event handlers
// ---------------------------------------------------------------------------
function onFiltersApply(filters: FilterSpec[]) {
  activeFilters.value = filters;
}

function onFiltersClear() {
  activeFilters.value = [];
  filteredRowCount.value = 0;
}

function onColumnsReset() {
  initColumnVisibility();
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
        <span class="row-count">
          <template v-if="activeFilters.length > 0">
            {{ filteredRowCount.toLocaleString() }} / {{ fileInfo!.total_rows.toLocaleString() }} rows
          </template>
          <template v-else>
            {{ fileInfo!.total_rows.toLocaleString() }} rows
          </template>
        </span>
        <button
          class="toolbar-btn filters-btn"
          :class="{ active: filterPanelOpen }"
          @click="filterPanelOpen = !filterPanelOpen"
        >
          Filters<span v-if="activeFilters.length > 0" class="badge">{{ activeFilters.length }}</span>
        </button>
        <button
          class="toolbar-btn columns-btn"
          :class="{ active: columnPanelOpen }"
          @click="columnPanelOpen = !columnPanelOpen"
        >
          Columns<span v-if="hiddenColumnCount > 0" class="badge">{{ hiddenColumnCount }}</span>
        </button>
        <div v-if="hasGeoColumns" class="view-toggle">
          <button :class="{ active: currentView === 'table' }" @click="currentView = 'table'">Table</button>
          <button :class="{ active: currentView === 'map' }" @click="currentView = 'map'">Map</button>
        </div>
        <button
          class="toolbar-btn"
          :disabled="exportState === 'exporting'"
          @click="exportFile"
        >{{ exportState === "exporting" ? "Exporting…" : "Export" }}</button>
        <button class="toolbar-btn" @click="openFile">Open File</button>
      </div>

      <FilterPanel
        v-show="filterPanelOpen"
        :columns="fileInfo!.columns"
        @apply="onFiltersApply"
        @clear="onFiltersClear"
      />
      <SelectPanel
        v-show="columnPanelOpen"
        :columns="fileInfo!.columns"
        :activeColumnVisibility="activeColumnVisibility"
        @apply="activeColumnVisibility = $event"
        @reset="onColumnsReset"
      />
      <DataGrid
        v-show="currentView === 'table'"
        :columns="fileInfo!.columns"
        :activeFilters="activeFilters"
        :activeColumnVisibility="activeColumnVisibility"
        @ready="gridApi = $event"
        @row-count-changed="filteredRowCount = $event"
      />
      <MapView
        v-if="hasGeoColumns"
        v-show="currentView === 'map'"
        :active="currentView === 'map'"
        :activeFilters="activeFilters"
        :latColumn="latColumn!"
        :lonColumn="lonColumn!"
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
  padding: 4px 14px;
  font-size: 0.8rem;
}

.toolbar-btn:last-child {
  margin-left: auto;
}

.filters-btn,
.columns-btn {
  background: transparent;
  color: #444;
  border: 1px solid #d0d0d0;
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.filters-btn:hover:not(:disabled),
.columns-btn:hover:not(:disabled) {
  background: #f0f0f0;
}

.filters-btn.active,
.columns-btn.active {
  background: #efefff;
  color: #646cff;
  border-color: #646cff;
}

.badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: #646cff;
  color: #fff;
  font-size: 0.7rem;
  font-weight: 700;
  min-width: 16px;
  height: 16px;
  border-radius: 8px;
  padding: 0 4px;
}

/* ---- View toggle ---- */
.view-toggle {
  display: flex;
  flex-shrink: 0;
}

.view-toggle button {
  padding: 4px 12px;
  font-size: 0.8rem;
  background: #fff;
  color: #444;
  border: 1px solid #d0d0d0;
  border-radius: 0;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.view-toggle button:first-child {
  border-radius: 6px 0 0 6px;
}

.view-toggle button:last-child {
  border-radius: 0 6px 6px 0;
  border-left: none;
}

.view-toggle button:hover:not(.active) {
  background: #f0f0f0;
}

.view-toggle button.active {
  background: #646cff;
  color: #fff;
  border-color: #646cff;
}
</style>
