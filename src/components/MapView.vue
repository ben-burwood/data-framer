<script setup lang="ts">
import { ref, watch, onUnmounted, nextTick } from "vue";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { invoke } from "@tauri-apps/api/core";
import type { FilterSpec } from "../types";

const props = defineProps<{
  active: boolean;
  activeFilters: FilterSpec[];
  latColumn: string;
  lonColumn: string;
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

// ---------------------------------------------------------------------------
// Map data
// ---------------------------------------------------------------------------
// fit=true: no bbox, load all points then fit map to data extent
// fit=false: use current viewport bounds
async function loadMapPoints(fit = false) {
  const bounds = fit ? null : mapInstance?.getBounds();
  mapLoading.value = true;
  try {
    const points = await invoke<[number, number][]>("get_map_points", {
      latCol: props.latColumn,
      lonCol: props.lonColumn,
      filters: props.activeFilters,
      minLat: bounds?.getSouth() ?? null,
      maxLat: bounds?.getNorth() ?? null,
      minLon: bounds?.getWest() ?? null,
      maxLon: bounds?.getEast() ?? null,
    });
    setMapPoints(points);
    if (fit && points.length > 0) fitMapToBounds(points);
  } finally {
    mapLoading.value = false;
  }
}

function setMapPoints(points: [number, number][]) {
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

function fitMapToBounds(points: [number, number][]) {
  let minLat = Infinity, maxLat = -Infinity, minLon = Infinity, maxLon = -Infinity;
  for (const [lat, lon] of points) {
    if (lat < minLat) minLat = lat;
    if (lat > maxLat) maxLat = lat;
    if (lon < minLon) minLon = lon;
    if (lon > maxLon) maxLon = lon;
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

function initMap() {
  if (!mapContainer.value) return;
  mapInstance = new maplibregl.Map({
    container: mapContainer.value,
    style: OSM_STYLE,
    center: [-2.5, 54.5],
    zoom: 5,
  });

  mapInstance.on("load", () => {
    mapInstance!.addSource("points", {
      type: "geojson",
      data: { type: "FeatureCollection", features: [] },
    });
    mapInstance!.addLayer(POINTS_LAYER);
    mapReady = true;
    void loadMapPoints(true);
  });

  let moveTimer: ReturnType<typeof setTimeout> | null = null;
  mapInstance.on("moveend", () => {
    if (suppressMoveHandler) return;
    if (moveTimer) clearTimeout(moveTimer);
    moveTimer = setTimeout(() => loadMapPoints(), 300);
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
      void loadMapPoints();
    }
  }
});

watch(() => props.activeFilters, () => {
  if (props.active && mapReady) void loadMapPoints(true);
}, { deep: true });

onUnmounted(() => {
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
