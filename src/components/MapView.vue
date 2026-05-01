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

const EMPTY_FC: maplibregl.GeoJSONSourceSpecification["data"] = {
  type: "FeatureCollection",
  features: [],
};

// ---------------------------------------------------------------------------
// Lat/lon points
// ---------------------------------------------------------------------------
// Returns raw [lat, lon] pairs from the backend (bbox-filtered when fit=false).
async function fetchLatLonPoints(fit: boolean): Promise<[number, number][]> {
  const bounds = fit ? null : mapInstance?.getBounds();
  return invoke<[number, number][]>("get_map_points", {
    latCol: props.latColumn,
    lonCol: props.lonColumn,
    filters: props.activeFilters,
    minLat: bounds?.getSouth() ?? null,
    maxLat: bounds?.getNorth() ?? null,
    minLon: bounds?.getWest() ?? null,
    maxLon: bounds?.getEast() ?? null,
  });
}

function setLatLonPoints(points: [number, number][]) {
  const source = mapInstance?.getSource("points") as maplibregl.GeoJSONSource | undefined;
  source?.setData({
    type: "FeatureCollection",
    features: points.map(([lat, lon]) => ({
      type: "Feature",
      geometry: { type: "Point", coordinates: [lon, lat] },
      properties: {},
    })),
  });
}

// ---------------------------------------------------------------------------
// H3 cells
// ---------------------------------------------------------------------------
// Returns polygon vertex coords in [lon, lat] GeoJSON order (for fitMapToBounds).
async function loadH3Cells(): Promise<[number, number][]> {
  const values = await invoke<string[]>("get_h3_values", {
    h3Col: props.h3Column,
    filters: props.activeFilters,
  });

  const allVerts: [number, number][] = [];
  const features: GeoJSON.Feature[] = values.map(cell => {
    // cellToBoundary returns [[lat, lng], ...] — swap to [lng, lat] for GeoJSON
    const boundary = cellToBoundary(cell);
    const ring = boundary.map(([lat, lng]) => [lng, lat] as [number, number]);
    ring.push(ring[0]);
    allVerts.push(...ring);
    return {
      type: "Feature",
      geometry: { type: "Polygon", coordinates: [ring] },
      properties: {},
    };
  });

  const source = mapInstance?.getSource("h3-cells") as maplibregl.GeoJSONSource | undefined;
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
  mapLoading.value = true;
  try {
    const [pts, verts] = await Promise.all([
      props.latColumn && props.lonColumn ? fetchLatLonPoints(fit) : Promise.resolve([] as [number, number][]),
      props.h3Column ? loadH3Cells() : Promise.resolve([] as [number, number][]),
    ]);

    if (props.latColumn && props.lonColumn) setLatLonPoints(pts);

    if (fit) {
      const allCoords: [number, number][] = [
        ...pts.map(([lat, lon]) => [lon, lat] as [number, number]),
        ...verts,
      ];
      if (allCoords.length > 0) fitMapToBoundsCoords(allCoords);
    }
  } finally {
    mapLoading.value = false;
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
    mapReady = true;
    void loadAll(true);
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
