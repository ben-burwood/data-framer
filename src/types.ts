export type Dtype =
  | "integer" | "float" | "boolean" | "date" | "datetime" | "string"
  | "decimal" | "time" | "duration" | "categorical" | "binary"
  | "list" | "struct";

export interface ColumnInfo {
  name: string;
  dtype: Dtype;
}

export interface FileInfo {
  path: string;
  total_rows: number;
  columns: ColumnInfo[];
}

export interface FilterSpec {
  column: string;
  op: string;
  value: string;
  value2: string;
}

export interface RowsResponse {
  rows: Record<string, unknown>[];
  total_rows: number;
}

export interface ChartConfig {
  chartType: "line" | "scatter";
  xColumn: string;
  yColumns: string[];
}
