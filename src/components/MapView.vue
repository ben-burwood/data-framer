<script setup lang="ts">
import { ref, watch, onUnmounted, nextTick } from "vue";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { cellToBoundary } from "h3-js";
import { invoke } from "@tauri-apps/api/core";
import type { FilterSpec } from "../types";

const props = defineProps<{
  active: boolean;
  activeFilters: FilterSpec[];
  latColumn: string | null;
  lonColumn: string | null;
  h3Column: string | null;
  geomColumn: string | null;
}>();

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
const mapContainer = ref<HTMLElement | null>(null);
// mapInstance and mapReady are plain variables — they're not used in the template,
// and wrapping maplibregl.Map in a Vue ref triggers TS2589 deep type instantiation.
let mapInstance: maplibregl.Map | null = null;
let mapReady = false;
const mapLoading = ref(false);
let suppressMoveHandler = false;
let moveTimer: ReturnType<typeof setTimeout> | null = null;
let popup: maplibregl.Popup | null = null;

const EMPTY_FC: maplibregl.GeoJSONSourceSpecification["data"] = {
  type: "FeatureCollection",
  features: [],
};

// Backend map-feature shapes. Each carries `idx` — the row's absolute position
// in the source file — so a click can fetch just that row's data (see get_row).
interface MapPoint { lat: number; lon: number; idx: number; }
interface H3Feature { cell: string; idx: number; }
interface GeomFeature { geometry: GeoJSON.Geometry; idx: number; }

// ---------------------------------------------------------------------------
// Lat/lon points
// ---------------------------------------------------------------------------
// Returns points from the backend (bbox-filtered when fit=false).
async function fetchLatLonPoints(fit: boolean): Promise<MapPoint[]> {
  const bounds = fit ? null : mapInstance?.getBounds();
  return invoke<MapPoint[]>("get_map_points", {
    latCol: props.latColumn,
    lonCol: props.lonColumn,
    filters: props.activeFilters,
    minLat: bounds?.getSouth() ?? null,
    maxLat: bounds?.getNorth() ?? null,
    minLon: bounds?.getWest() ?? null,
    maxLon: bounds?.getEast() ?? null,
  });
}

function setLatLonPoints(points: MapPoint[]) {
  const source = mapInstance?.getSource("points") as maplibregl.GeoJSONSource | undefined;
  source?.setData({
    type: "FeatureCollection",
    features: points.map(p => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [p.lon, p.lat] },
      properties: { __idx: p.idx },
    })),
  });
}

// ---------------------------------------------------------------------------
// H3 cells
// ---------------------------------------------------------------------------
// Returns polygon vertex coords in [lon, lat] GeoJSON order (for fitMapToBounds).
async function loadH3Cells(): Promise<[number, number][]> {
  const cells = await invoke<H3Feature[]>("get_h3_values", {
    h3Col: props.h3Column,
    filters: props.activeFilters,
  });

  const allVerts: [number, number][] = [];
  const features: GeoJSON.Feature[] = cells.map(({ cell, idx }) => {
    // cellToBoundary returns [[lat, lng], ...] — swap to [lng, lat] for GeoJSON
    const boundary = cellToBoundary(cell);
    const ring = boundary.map(([lat, lng]) => [lng, lat] as [number, number]);
    ring.push(ring[0]);
    allVerts.push(...ring);
    return {
      type: "Feature",
      geometry: { type: "Polygon", coordinates: [ring] },
      properties: { __idx: idx },
    };
  });

  const source = mapInstance?.getSource("h3-cells") as maplibregl.GeoJSONSource | undefined;
  source?.setData({ type: "FeatureCollection", features });
  return allVerts;
}

// ---------------------------------------------------------------------------
// WKB geometry (GeoParquet)
// ---------------------------------------------------------------------------
// Recursively collect every [lon, lat] position out of a GeoJSON coordinates
// array of any nesting depth (Point → MultiPolygon), for fit-to-bounds.
function collectCoords(node: unknown, out: [number, number][]) {
  if (Array.isArray(node) && typeof node[0] === "number") {
    out.push([node[0], node[1] as number]);
  } else if (Array.isArray(node)) {
    for (const child of node) collectCoords(child, out);
  }
}

// Fetches decoded GeoJSON geometries, renders them, and returns their vertices
// as [lon, lat] pairs (GeoJSON order) for fitMapToBounds.
async function loadGeometry(): Promise<[number, number][]> {
  const feats = await invoke<GeomFeature[]>("get_geometry", {
    geomCol: props.geomColumn,
    filters: props.activeFilters,
  });

  const allVerts: [number, number][] = [];
  const features: GeoJSON.Feature[] = feats.map(({ geometry, idx }) => {
    collectCoords((geometry as { coordinates?: unknown }).coordinates, allVerts);
    return { type: "Feature", geometry, properties: { __idx: idx } };
  });

  const source = mapInstance?.getSource("geometry") as maplibregl.GeoJSONSource | undefined;
  source?.setData({ type: "FeatureCollection", features });
  return allVerts;
}

// ---------------------------------------------------------------------------
// Fit map to data extent — accepts [lon, lat] pairs (GeoJSON order)
// ---------------------------------------------------------------------------
function fitMapToBoundsCoords(coords: [number, number][]) {
  let minLon = Infinity, maxLon = -Infinity, minLat = Infinity, maxLat = -Infinity;
  for (const [lon, lat] of coords) {
    if (lon < minLon) minLon = lon;
    if (lon > maxLon) maxLon = lon;
    if (lat < minLat) minLat = lat;
    if (lat > maxLat) maxLat = lat;
  }
  suppressMoveHandler = true;
  // Use once('moveend') so the flag is cleared exactly when the animation ends,
  // not on an arbitrary timeout that might race with user panning.
  mapInstance!.once("moveend", () => { suppressMoveHandler = false; });
  mapInstance!.fitBounds(
    [[minLon, minLat], [maxLon, maxLat]],
    { padding: 40, duration: 500 },
  );
}

// ---------------------------------------------------------------------------
// Unified load — runs whichever layers are configured
// ---------------------------------------------------------------------------
async function loadAll(fit = false) {
  // A reload can change or remove the feature the popup points at.
  popup?.remove();
  popup = null;
  mapLoading.value = true;
  try {
    const [pts, verts, geomVerts] = await Promise.all([
      props.latColumn && props.lonColumn ? fetchLatLonPoints(fit) : Promise.resolve([] as MapPoint[]),
      props.h3Column ? loadH3Cells() : Promise.resolve([] as [number, number][]),
      props.geomColumn ? loadGeometry() : Promise.resolve([] as [number, number][]),
    ]);

    if (props.latColumn && props.lonColumn) setLatLonPoints(pts);

    if (fit) {
      const allCoords: [number, number][] = [
        ...pts.map(p => [p.lon, p.lat] as [number, number]),
        ...verts,
        ...geomVerts,
      ];
      if (allCoords.length > 0) fitMapToBoundsCoords(allCoords);
    }
  } finally {
    mapLoading.value = false;
  }
}

// ---------------------------------------------------------------------------
// Feature popup — shows the clicked feature's full source row
// ---------------------------------------------------------------------------
// The interactive layers whose features open a popup, filtered to those that
// actually exist for the current file.
function popupLayers(): string[] {
  return ["points", "h3-fill", "geometry-fill", "geometry-outline", "geometry-points"]
    .filter(id => mapInstance?.getLayer(id));
}

const HTML_ESCAPES: Record<string, string> = {
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
};
function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, c => HTML_ESCAPES[c]);
}

// Render a row object as a two-column key/value table.
function renderPopupHtml(row: Record<string, unknown>): string {
  const body = Object.entries(row)
    .map(([k, v]) => {
      const display =
        v === null || v === undefined ? ""
        : typeof v === "object" ? JSON.stringify(v)
        : String(v);
      return `<tr><th>${escapeHtml(k)}</th><td>${escapeHtml(display)}</td></tr>`;
    })
    .join("");
  return `<div class="map-popup"><table>${body}</table></div>`;
}

// Open a popup at the click point, then fetch and render just that row.
async function showFeaturePopup(lngLat: maplibregl.LngLatLike, idx: number) {
  popup?.remove();
  popup = new maplibregl.Popup({ maxWidth: "340px" })
    .setLngLat(lngLat)
    .setHTML(`<div class="map-popup map-popup-status">Loading…</div>`)
    .addTo(mapInstance!);
  const opened = popup;
  try {
    const row = await invoke<Record<string, unknown> | null>("get_row", { index: idx });
    const data = { ...(row ?? {}) };
    // The geometry column is raw WKB bytes — not useful in the popup.
    if (props.geomColumn) delete data[props.geomColumn];
    // The popup may have been closed/replaced while the fetch was in flight.
    if (popup === opened) popup.setHTML(renderPopupHtml(data));
  } catch (err) {
    if (popup === opened) {
      popup.setHTML(`<div class="map-popup map-popup-status">Failed to load row: ${escapeHtml(String(err))}</div>`);
    }
  }
}

// ---------------------------------------------------------------------------
// Map init
// ---------------------------------------------------------------------------
// Typed constant avoids "excessively deep" TS2589 from MapLibre's recursive style types.
const OSM_STYLE: maplibregl.StyleSpecification = {
  version: 8,
  sources: {
    osm: {
      type: "raster",
      tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
      tileSize: 256,
      attribution: "© OpenStreetMap contributors",
    },
  },
  layers: [{ id: "osm", type: "raster", source: "osm" }],
};

const POINTS_LAYER: maplibregl.CircleLayerSpecification = {
  id: "points",
  type: "circle",
  source: "points",
  paint: {
    "circle-radius": 5,
    "circle-color": "#646cff",
    "circle-opacity": 0.7,
  },
};

const H3_FILL_LAYER: maplibregl.FillLayerSpecification = {
  id: "h3-fill",
  type: "fill",
  source: "h3-cells",
  paint: {
    "fill-color": "#ff9800",
    "fill-opacity": 0.35,
  },
};

const H3_LINE_LAYER: maplibregl.LineLayerSpecification = {
  id: "h3-outline",
  type: "line",
  source: "h3-cells",
  paint: {
    "line-color": "#ff9800",
    "line-width": 1,
    "line-opacity": 0.8,
  },
};

// Three layers on one source cover any geometry type: fill renders polygon
// interiors, line renders LineStrings and polygon outlines, circle renders Points.
const GEOM_FILL_LAYER: maplibregl.FillLayerSpecification = {
  id: "geometry-fill",
  type: "fill",
  source: "geometry",
  filter: ["match", ["geometry-type"], ["Polygon", "MultiPolygon"], true, false],
  paint: {
    "fill-color": "#646cff",
    "fill-opacity": 0.25,
  },
};

const GEOM_LINE_LAYER: maplibregl.LineLayerSpecification = {
  id: "geometry-outline",
  type: "line",
  source: "geometry",
  paint: {
    "line-color": "#4149c9",
    "line-width": 1,
    "line-opacity": 0.9,
  },
};

const GEOM_CIRCLE_LAYER: maplibregl.CircleLayerSpecification = {
  id: "geometry-points",
  type: "circle",
  source: "geometry",
  filter: ["match", ["geometry-type"], ["Point", "MultiPoint"], true, false],
  paint: {
    "circle-radius": 5,
    "circle-color": "#646cff",
    "circle-opacity": 0.7,
  },
};

function initMap() {
  if (!mapContainer.value) return;
  mapInstance = new maplibregl.Map({
    container: mapContainer.value,
    style: OSM_STYLE,
    center: [-2.5, 54.5],
    zoom: 5,
  });

  mapInstance.on("load", () => {
    if (props.latColumn && props.lonColumn) {
      mapInstance!.addSource("points", { type: "geojson", data: EMPTY_FC });
      mapInstance!.addLayer(POINTS_LAYER);
    }
    if (props.h3Column) {
      mapInstance!.addSource("h3-cells", { type: "geojson", data: EMPTY_FC });
      mapInstance!.addLayer(H3_FILL_LAYER);
      mapInstance!.addLayer(H3_LINE_LAYER);
    }
    if (props.geomColumn) {
      mapInstance!.addSource("geometry", { type: "geojson", data: EMPTY_FC });
      mapInstance!.addLayer(GEOM_FILL_LAYER);
      mapInstance!.addLayer(GEOM_LINE_LAYER);
      mapInstance!.addLayer(GEOM_CIRCLE_LAYER);
    }
    mapReady = true;
    void loadAll(true);
  });

  // Click a feature → popup with its row data. A single map-level handler
  // (rather than per-layer) avoids firing twice where fill/outline overlap.
  mapInstance.on("click", (e) => {
    const layers = popupLayers();
    if (layers.length === 0) return;
    const feats = mapInstance!.queryRenderedFeatures(e.point, { layers });
    const idx = feats[0]?.properties?.__idx;
    if (typeof idx !== "number") return;
    void showFeaturePopup(e.lngLat, idx);
  });

  // Pointer cursor when hovering a clickable feature.
  mapInstance.on("mousemove", (e) => {
    const layers = popupLayers();
    const over = layers.length > 0 && mapInstance!.queryRenderedFeatures(e.point, { layers }).length > 0;
    mapInstance!.getCanvas().style.cursor = over ? "pointer" : "";
  });

  mapInstance.on("moveend", () => {
    if (suppressMoveHandler) return;
    if (moveTimer) clearTimeout(moveTimer);
    // Only reload lat/lon on viewport change (H3 loads all rows, no bbox)
    moveTimer = setTimeout(() => {
      if (!props.latColumn || !props.lonColumn) return;
      mapLoading.value = true;
      fetchLatLonPoints(false)
        .then(pts => setLatLonPoints(pts))
        .finally(() => { mapLoading.value = false; });
    }, 300);
  });
}

// ---------------------------------------------------------------------------
// React to prop changes
// ---------------------------------------------------------------------------
watch(() => props.active, async (active) => {
  if (active) {
    await nextTick();
    if (!mapInstance) {
      initMap();
    } else if (mapReady) {
      void loadAll();
    }
  }
});

watch(() => props.activeFilters, () => {
  if (props.active && mapReady) void loadAll(true);
}, { deep: true });

onUnmounted(() => {
  if (moveTimer) clearTimeout(moveTimer);
  popup?.remove();
  popup = null;
  mapInstance?.remove();
  mapInstance = null;
});
</script>

<template>
  <div class="map-wrapper" :class="{ fetching: mapLoading }">
    <div ref="mapContainer" class="map-container" />
  </div>
</template>

<style scoped>
.map-wrapper {
  flex: 1;
  min-height: 0;
  position: relative;
  overflow: hidden;
}

.map-container {
  width: 100%;
  height: 100%;
}
</style>

<!-- Not scoped: MapLibre renders popup DOM outside this component's tree. -->
<style>
.maplibregl-popup-content {
  max-height: 320px;
  overflow: auto;
  padding: 10px 12px;
  font-family: var(--ag-font-family, "IBM Plex Sans", sans-serif);
}

.map-popup table {
  border-collapse: collapse;
  font-size: 12px;
}

.map-popup th,
.map-popup td {
  text-align: left;
  padding: 2px 0;
  vertical-align: top;
}

.map-popup th {
  color: #646c7a;
  font-weight: 600;
  white-space: nowrap;
  padding-right: 10px;
}

.map-popup td {
  color: #181d1f;
  word-break: break-word;
}

.map-popup-status {
  font-size: 12px;
  color: #646c7a;
}
</style>
