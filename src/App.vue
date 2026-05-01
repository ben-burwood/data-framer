<script setup lang="ts">
import { ref, computed } from "vue";
import { AgGridVue } from "ag-grid-vue3";
import { ModuleRegistry, AllCommunityModule } from "ag-grid-community";
import type {
  ColDef,
  FirstDataRenderedEvent,
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
// Filter types
// ---------------------------------------------------------------------------
type FilterOp =
  | "eq" | "neq"
  | "gt" | "gte" | "lt" | "lte" | "between"
  | "contains" | "not_contains" | "starts_with" | "ends_with"
  | "is_true" | "is_false"
  | "is_null" | "is_not_null";

interface FilterSpec {
  column: string;
  op: FilterOp;
  value: string;
  value2: string;
}

interface OpDef {
  label: string;
  op: FilterOp;
  hasValue: boolean;
  hasTwoValues: boolean;
}

const NULL_OPS: OpDef[] = [
  { label: "is null",     op: "is_null",     hasValue: false, hasTwoValues: false },
  { label: "is not null", op: "is_not_null", hasValue: false, hasTwoValues: false },
];

const NUMERIC_OPS: OpDef[] = [
  { label: "equals",                op: "eq",      hasValue: true,  hasTwoValues: false },
  { label: "not equals",            op: "neq",     hasValue: true,  hasTwoValues: false },
  { label: "greater than",          op: "gt",      hasValue: true,  hasTwoValues: false },
  { label: "greater than or equal", op: "gte",     hasValue: true,  hasTwoValues: false },
  { label: "less than",             op: "lt",      hasValue: true,  hasTwoValues: false },
  { label: "less than or equal",    op: "lte",     hasValue: true,  hasTwoValues: false },
  { label: "between",               op: "between", hasValue: true,  hasTwoValues: true  },
  ...NULL_OPS,
];

const DATE_OPS: OpDef[] = [
  { label: "equals",       op: "eq",      hasValue: true,  hasTwoValues: false },
  { label: "not equals",   op: "neq",     hasValue: true,  hasTwoValues: false },
  { label: "after",        op: "gt",      hasValue: true,  hasTwoValues: false },
  { label: "after or on",  op: "gte",     hasValue: true,  hasTwoValues: false },
  { label: "before",       op: "lt",      hasValue: true,  hasTwoValues: false },
  { label: "before or on", op: "lte",     hasValue: true,  hasTwoValues: false },
  { label: "between",      op: "between", hasValue: true,  hasTwoValues: true  },
  ...NULL_OPS,
];

const OPS_BY_DTYPE: Record<Dtype, OpDef[]> = {
  string: [
    { label: "equals",       op: "eq",          hasValue: true,  hasTwoValues: false },
    { label: "not equals",   op: "neq",         hasValue: true,  hasTwoValues: false },
    { label: "contains",     op: "contains",    hasValue: true,  hasTwoValues: false },
    { label: "not contains", op: "not_contains",hasValue: true,  hasTwoValues: false },
    { label: "starts with",  op: "starts_with", hasValue: true,  hasTwoValues: false },
    { label: "ends with",    op: "ends_with",   hasValue: true,  hasTwoValues: false },
    ...NULL_OPS,
  ],
  integer:  NUMERIC_OPS,
  float:    NUMERIC_OPS,
  boolean: [
    { label: "is true",  op: "is_true",  hasValue: false, hasTwoValues: false },
    { label: "is false", op: "is_false", hasValue: false, hasTwoValues: false },
    ...NULL_OPS,
  ],
  date:     DATE_OPS,
  datetime: DATE_OPS,
};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
type ViewState = "empty" | "loading" | "loaded";

const view = ref<ViewState>("empty");
const fileInfo = ref<FileInfo | null>(null);
const gridApi = ref<GridApi | null>(null);
const showFilters = ref(false);
const pendingFilters = ref<FilterSpec[]>([]);
const activeFilters = ref<FilterSpec[]>([]);
const filteredRowCount = ref(0);

const showColumns             = ref(false);
const pendingColumnVisibility = ref<Record<string, boolean>>({});
const activeColumnVisibility  = ref<Record<string, boolean>>({});

const visibleColumns = computed(() =>
  fileInfo.value?.columns.filter(c => activeColumnVisibility.value[c.name] !== false) ?? []
);

const columnDefs = computed<ColDef[]>(() => schemaToColDefs(visibleColumns.value));

const fileName = computed(() => fileInfo.value?.path.split(/[\\/]/).pop() ?? "");

// Empty array = all columns; backend skips the .select() call entirely.
const selectedColumnNames = computed(() => {
  if (!fileInfo.value) return [];
  const visible = visibleColumns.value.map(c => c.name);
  return visible.length < fileInfo.value.columns.length ? visible : [];
});

const hiddenColumnCount = computed(() =>
  (fileInfo.value?.columns.length ?? 0) - visibleColumns.value.length
);

const defaultColDef: ColDef = {
  minWidth: 80,
  sortable: true,
  resizable: true,
};

// ---------------------------------------------------------------------------
// Filter helpers
// ---------------------------------------------------------------------------
function opDefsForFilter(f: FilterSpec): OpDef[] {
  const col = fileInfo.value?.columns.find((c) => c.name === f.column);
  return col ? OPS_BY_DTYPE[col.dtype] : [];
}

function currentOpDef(f: FilterSpec): OpDef | undefined {
  return opDefsForFilter(f).find((d) => d.op === f.op);
}

function onColumnChange(f: FilterSpec) {
  const defs = opDefsForFilter(f);
  if (defs.length > 0) {
    f.op = defs[0].op;
    f.value = "";
    f.value2 = "";
  }
}

function inputTypeForFilter(f: FilterSpec): string {
  const col = fileInfo.value?.columns.find((c) => c.name === f.column);
  switch (col?.dtype) {
    case "integer":
    case "float":    return "number";
    case "date":     return "date";
    case "datetime": return "datetime-local";
    default:         return "text";
  }
}

function addFilter() {
  const firstCol = fileInfo.value?.columns[0];
  if (!firstCol) return;
  pendingFilters.value.push({
    column: firstCol.name,
    op: OPS_BY_DTYPE[firstCol.dtype][0].op,
    value: "",
    value2: "",
  });
}

function removeFilter(idx: number) {
  pendingFilters.value.splice(idx, 1);
}

// datetime-local inputs give "YYYY-MM-DDTHH:MM" but the backend needs seconds.
function normalizeDateTime(value: string): string {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value) ? value + ":00" : value;
}

function applyFilters() {
  // Validate: all filters with a value input must have a non-empty value.
  for (const f of pendingFilters.value) {
    const def = currentOpDef(f);
    if (!def) continue;
    // String() coercion needed: <input type="number"> v-model returns a JS number, not a string.
    if ((def.hasValue && !String(f.value ?? "").trim()) || (def.hasTwoValues && !String(f.value2 ?? "").trim())) {
      alert("Please fill in all filter values before applying.");
      return;
    }
  }

  activeFilters.value = pendingFilters.value.map((f) => {
    const dtype = fileInfo.value?.columns.find((c) => c.name === f.column)?.dtype ?? "string";
    // Stringify: number inputs yield JS numbers; Rust FilterSpec expects strings.
    const val  = String(f.value  ?? "");
    const val2 = String(f.value2 ?? "");
    const isDatetime = dtype === "datetime";
    return {
      ...f,
      value:  isDatetime ? normalizeDateTime(val)  : val,
      value2: isDatetime ? normalizeDateTime(val2) : val2,
    };
  });

  gridApi.value?.purgeInfiniteCache();
}

function clearFilters() {
  pendingFilters.value = [];
  activeFilters.value = [];
  filteredRowCount.value = 0;
  gridApi.value?.purgeInfiniteCache();
}

// ---------------------------------------------------------------------------
// Column visibility helpers
// ---------------------------------------------------------------------------
function initColumnVisibility(cols: ColumnInfo[]) {
  const vis: Record<string, boolean> = {};
  for (const c of cols) vis[c.name] = true;
  pendingColumnVisibility.value = vis;           // direct ownership
  activeColumnVisibility.value  = { ...vis };    // independent copy
}

function togglePendingColumn(name: string) {
  pendingColumnVisibility.value[name] = !pendingColumnVisibility.value[name];
}

function selectAllColumns() {
  pendingColumnVisibility.value = Object.fromEntries(
    fileInfo.value!.columns.map(c => [c.name, true])
  );
}

function deselectAllColumns() {
  pendingColumnVisibility.value = Object.fromEntries(
    fileInfo.value!.columns.map(c => [c.name, false])
  );
}

function applyColumns() {
  const anyVisible = fileInfo.value?.columns.some(
    c => pendingColumnVisibility.value[c.name] !== false
  );
  if (!anyVisible) { alert("At least one column must be visible."); return; }
  const changed = fileInfo.value!.columns.some(
    c => pendingColumnVisibility.value[c.name] !== activeColumnVisibility.value[c.name]
  );
  if (!changed) return;
  activeColumnVisibility.value = { ...pendingColumnVisibility.value };
  gridApi.value?.purgeInfiniteCache();
}

function resetColumns() {
  const alreadyDefault = fileInfo.value!.columns.every(
    c => activeColumnVisibility.value[c.name] !== false
  );
  initColumnVisibility(fileInfo.value!.columns);
  if (!alreadyDefault) gridApi.value?.purgeInfiniteCache();
}

// ---------------------------------------------------------------------------
// Grid helpers
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
        filters: activeFilters.value,
        columns: selectedColumnNames.value,
      })
        .then((r) => {
          if (filteredRowCount.value !== r.total_rows) {
            filteredRowCount.value = r.total_rows;
          }
          params.successCallback(r.rows, r.total_rows);
        })
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
  if (fileInfo.value) {
    event.api.setGridOption("datasource", buildDatasource());
  }
}

function onFirstDataRendered(event: FirstDataRenderedEvent) {
  event.api.autoSizeAllColumns();
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
  showFilters.value = false;
  pendingFilters.value = [];
  activeFilters.value = [];
  filteredRowCount.value = 0;
  showColumns.value = false;

  try {
    const info = await invoke<FileInfo>("load_file", { path });
    fileInfo.value = info;
    initColumnVisibility(info.columns);
    view.value = "loaded";
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
          :class="{ active: showFilters }"
          @click="showFilters = !showFilters"
        >
          Filters<span v-if="activeFilters.length > 0" class="badge">{{ activeFilters.length }}</span>
        </button>
        <button
          class="toolbar-btn columns-btn"
          :class="{ active: showColumns }"
          @click="showColumns = !showColumns"
        >
          Columns<span v-if="hiddenColumnCount > 0" class="badge">{{ hiddenColumnCount }}</span>
        </button>
        <button class="toolbar-btn" @click="openFile">Open File</button>
      </div>

      <!-- Filter panel -->
      <div v-if="showFilters" class="filter-panel">
        <div v-for="(f, i) in pendingFilters" :key="i" class="filter-row">
          <select v-model="f.column" @change="onColumnChange(f)" class="filter-select">
            <option v-for="col in fileInfo!.columns" :key="col.name" :value="col.name">
              {{ col.name }}
            </option>
          </select>
          <select v-model="f.op" class="filter-select op-select">
            <option v-for="def in opDefsForFilter(f)" :key="def.op" :value="def.op">
              {{ def.label }}
            </option>
          </select>
          <input
            v-if="currentOpDef(f)?.hasValue"
            v-model="f.value"
            :type="inputTypeForFilter(f)"
            class="filter-value"
            placeholder="value"
          />
          <input
            v-if="currentOpDef(f)?.hasTwoValues"
            v-model="f.value2"
            :type="inputTypeForFilter(f)"
            class="filter-value"
            placeholder="to"
          />
          <button class="remove-btn" @click="removeFilter(i)" title="Remove filter">×</button>
        </div>
        <div class="filter-actions">
          <button class="add-filter-btn" @click="addFilter">+ Add Filter</button>
          <div class="filter-action-btns">
            <button class="clear-btn" @click="clearFilters">Clear All</button>
            <button class="apply-btn" @click="applyFilters">Apply</button>
          </div>
        </div>
      </div>

      <!-- Column panel -->
      <div v-if="showColumns" class="column-panel">
        <div class="column-list">
          <label v-for="col in fileInfo!.columns" :key="col.name" class="column-item">
            <input
              type="checkbox"
              :checked="pendingColumnVisibility[col.name] !== false"
              @change="togglePendingColumn(col.name)"
            />
            <span class="col-item-name">{{ col.name }}</span>
            <span class="col-dtype-badge">{{ col.dtype }}</span>
          </label>
        </div>
        <div class="column-actions">
          <div class="filter-action-btns">
            <button class="add-filter-btn" @click="selectAllColumns">Select All</button>
            <button class="clear-btn" @click="deselectAllColumns">Deselect All</button>
          </div>
          <div class="filter-action-btns">
            <button class="clear-btn" @click="resetColumns">Reset</button>
            <button class="apply-btn" @click="applyColumns">Apply</button>
          </div>
        </div>
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
        @first-data-rendered="onFirstDataRendered"
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

/* ---- Filter panel ---- */
.filter-panel {
  padding: 8px 14px 4px;
  background: #fafafa;
  border-bottom: 1px solid #e0e0e0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-shrink: 0;
}

.filter-row {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.filter-select {
  padding: 4px 6px;
  font-size: 0.8rem;
  font-family: inherit;
  border: 1px solid #d0d0d0;
  border-radius: 6px;
  background: #fff;
  cursor: pointer;
}

.filter-select.op-select {
  min-width: 120px;
}

.filter-value {
  padding: 4px 8px;
  font-size: 0.8rem;
  font-family: inherit;
  border: 1px solid #d0d0d0;
  border-radius: 6px;
  width: 160px;
}

.remove-btn {
  padding: 2px 8px;
  font-size: 1rem;
  line-height: 1;
  background: transparent;
  color: #999;
  border: 1px solid #d0d0d0;
  border-radius: 6px;
}

.remove-btn:hover:not(:disabled) {
  background: #fee2e2;
  color: #b91c1c;
  border-color: #fca5a5;
}

.filter-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 0 6px;
}

.filter-action-btns {
  display: flex;
  gap: 6px;
}

.add-filter-btn {
  padding: 4px 12px;
  font-size: 0.8rem;
  background: transparent;
  color: #646cff;
  border: 1px dashed #646cff;
  border-radius: 6px;
}

.add-filter-btn:hover:not(:disabled) {
  background: #efefff;
}

.clear-btn {
  padding: 4px 12px;
  font-size: 0.8rem;
  background: transparent;
  color: #555;
  border: 1px solid #d0d0d0;
  border-radius: 6px;
}

.clear-btn:hover:not(:disabled) {
  background: #f0f0f0;
}

.apply-btn {
  padding: 4px 16px;
  font-size: 0.8rem;
  background: #646cff;
  color: #fff;
  border: none;
  border-radius: 6px;
}

.apply-btn:hover:not(:disabled) {
  background: #535bf2;
}

/* ---- Column panel ---- */
.column-panel {
  padding: 8px 14px 4px;
  background: #fafafa;
  border-bottom: 1px solid #e0e0e0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-shrink: 0;
}

.column-list {
  max-height: 240px;
  overflow-y: auto;
  display: flex;
  flex-wrap: wrap;
  gap: 4px 24px;
}

.column-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.82rem;
  cursor: pointer;
  min-width: 160px;
}

.col-item-name {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.col-dtype-badge {
  font-size: 0.7rem;
  color: #888;
  background: #f0f0f0;
  border-radius: 4px;
  padding: 1px 5px;
  white-space: nowrap;
}

.column-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 0 6px;
}

/* ---- Grid ---- */
.grid {
  flex: 1;
  min-height: 0;
}
</style>
