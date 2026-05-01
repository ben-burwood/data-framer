<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { AgGridVue } from "ag-grid-vue3";
import { ModuleRegistry, AllCommunityModule, themeQuartz } from "ag-grid-community";
import type {
  ColDef,
  FirstDataRenderedEvent,
  GridApi,
  GridReadyEvent,
  IDatasource,
  IGetRowsParams,
} from "ag-grid-community";
import { invoke } from "@tauri-apps/api/core";
import type { ColumnInfo, FilterSpec, RowsResponse } from "../types";

ModuleRegistry.registerModules([AllCommunityModule]);

const gridTheme = themeQuartz.withParams({
  headerBackgroundColor: "color-mix(in srgb, white, #2196f3 12%)",
});

const props = defineProps<{
  columns: ColumnInfo[];
  activeFilters: FilterSpec[];
  activeColumnVisibility: Record<string, boolean>;
}>();

const emit = defineEmits<{
  ready: [api: GridApi];
  "row-count-changed": [count: number];
}>();

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
const gridApi = ref<GridApi | null>(null);
const pendingFetches = ref(0);
const isFetching = computed(() => pendingFetches.value > 0);

// ---------------------------------------------------------------------------
// Column defs
// ---------------------------------------------------------------------------
const visibleColumns = computed(() =>
  props.columns.filter(c => props.activeColumnVisibility[c.name] !== false)
);

const columnDefs = computed<ColDef[]>(() =>
  visibleColumns.value.map((c) => ({
    field: c.name,
    headerName: c.name,
    ...(c.dtype === "integer" || c.dtype === "float" ? { type: "numericColumn" } : {}),
  }))
);

const selectedColumnNames = computed(() => {
  const visible = visibleColumns.value.map(c => c.name);
  return visible.length < props.columns.length ? visible : [];
});

const defaultColDef: ColDef = {
  minWidth: 80,
  sortable: true,
  resizable: true,
};

// ---------------------------------------------------------------------------
// Datasource
// ---------------------------------------------------------------------------
function buildDatasource(): IDatasource {
  return {
    async getRows(params: IGetRowsParams) {
      const { startRow, endRow, sortModel } = params;
      pendingFetches.value++;
      try {
        const r = await invoke<RowsResponse>("get_rows", {
          offset: startRow,
          limit: endRow - startRow,
          sortCol: sortModel[0]?.colId ?? null,
          sortDesc: sortModel[0]?.sort === "desc",
          filters: props.activeFilters,
          columns: selectedColumnNames.value,
        });
        emit("row-count-changed", r.total_rows);
        params.successCallback(r.rows, r.total_rows);
      } catch (err) {
        console.error("get_rows failed:", err);
        params.failCallback();
      } finally {
        pendingFetches.value--;
      }
    },
  };
}

// ---------------------------------------------------------------------------
// Grid events
// ---------------------------------------------------------------------------
function onGridReady(event: GridReadyEvent) {
  gridApi.value = event.api;
  event.api.setGridOption("datasource", buildDatasource());
  emit("ready", event.api);
}

function onFirstDataRendered(event: FirstDataRenderedEvent) {
  event.api.autoSizeAllColumns();
}

// ---------------------------------------------------------------------------
// React to prop changes
// ---------------------------------------------------------------------------
watch(
  [() => props.activeFilters, () => props.activeColumnVisibility],
  () => { gridApi.value?.purgeInfiniteCache(); },
  { deep: true },
);
</script>

<template>
  <div class="grid-wrapper" :class="{ fetching: isFetching }">
    <AgGridVue
      class="grid"
      :theme="gridTheme"
      :columnDefs="columnDefs"
      :defaultColDef="defaultColDef"
      rowModelType="infinite"
      :cacheBlockSize="200"
      :maxBlocksInCache="20"
      :infiniteInitialRowCount="1"
      @grid-ready="onGridReady"
      @first-data-rendered="onFirstDataRendered"
    />
  </div>
</template>

<style scoped>
.grid-wrapper {
  flex: 1;
  min-height: 0;
  position: relative;
}

.grid {
  width: 100%;
  height: 100%;
}

</style>
