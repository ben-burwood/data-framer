<script setup lang="ts">
import { ref } from "vue";
import { AgGridVue } from "ag-grid-vue3";
import { ModuleRegistry, AllCommunityModule } from "ag-grid-community";
import type { ColDef } from "ag-grid-community";
import "ag-grid-community/styles/ag-grid.css";
import "ag-grid-community/styles/ag-theme-quartz.css";

ModuleRegistry.registerModules([AllCommunityModule]);

interface RowData {
  id: number;
  name: string;
  category: string;
  value: number;
  currency: string;
  date: string;
  status: string;
}

const columnDefs = ref<ColDef<RowData>[]>([
  { field: "id", headerName: "ID", maxWidth: 80 },
  { field: "name", headerName: "Name" },
  { field: "category", headerName: "Category" },
  { field: "value", headerName: "Value", valueFormatter: (p) => p.value.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 }) },
  { field: "currency", headerName: "Currency", maxWidth: 110 },
  { field: "date", headerName: "Date" },
  { field: "status", headerName: "Status" },
]);

const defaultColDef = ref<ColDef>({
  flex: 1,
  sortable: true,
  filter: true,
  resizable: true,
});

const rowData = ref<RowData[]>([
  { id: 1,  name: "AAPL US Equity",   category: "Equity",      value: 189.34,  currency: "USD", date: "2026-04-28", status: "Active"   },
  { id: 2,  name: "MSFT US Equity",   category: "Equity",      value: 415.20,  currency: "USD", date: "2026-04-28", status: "Active"   },
  { id: 3,  name: "TSLA US Equity",   category: "Equity",      value: 172.55,  currency: "USD", date: "2026-04-28", status: "Active"   },
  { id: 4,  name: "US 10Y Treasury",  category: "Fixed Income", value: 98.67,  currency: "USD", date: "2026-04-25", status: "Settled"  },
  { id: 5,  name: "EUR/USD FX Fwd",   category: "FX",          value: 1.0823,  currency: "EUR", date: "2026-05-01", status: "Pending"  },
  { id: 6,  name: "Brent Crude Jul",  category: "Commodity",   value: 84.12,   currency: "USD", date: "2026-04-30", status: "Active"   },
  { id: 7,  name: "Gold Spot",        category: "Commodity",   value: 2315.40, currency: "USD", date: "2026-04-28", status: "Active"   },
  { id: 8,  name: "NVDA US Equity",   category: "Equity",      value: 887.60,  currency: "USD", date: "2026-04-28", status: "Active"   },
  { id: 9,  name: "GBP/JPY FX Spot",  category: "FX",          value: 192.34,  currency: "GBP", date: "2026-04-29", status: "Settled"  },
  { id: 10, name: "DE 5Y Bund",       category: "Fixed Income", value: 101.23, currency: "EUR", date: "2026-04-25", status: "Settled"  },
]);
</script>

<template>
  <AgGridVue
    class="ag-theme-quartz"
    :columnDefs="columnDefs"
    :rowData="rowData"
    :defaultColDef="defaultColDef"
  />
</template>

<style>
*, *::before, *::after {
  box-sizing: border-box;
}

html, body, #app {
  height: 100%;
  margin: 0;
  padding: 0;
  overflow: hidden;
}

.ag-theme-quartz {
  width: 100%;
  height: 100%;
}
</style>
