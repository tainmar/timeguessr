import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import { englishMapStyle } from "../lib/mapStyle";
import { boundsOf, type LngLat } from "../lib/geo";

export interface MapMarker {
  id: string;
  lng: number;
  lat: number;
  color: string;
  label?: string;
  title?: string;
}

export interface MapViewHandle {
  flyTo: (lng: number, lat: number, zoom?: number) => void;
  getZoom: () => number;
}

interface Props {
  markers?: MapMarker[];
  lines?: LngLat[][];
  /** Points to fit the view to (whenever this changes). */
  fitTo?: LngLat[] | null;
  initialCenter?: LngLat;
  initialZoom?: number;
  onPick?: (lng: number, lat: number) => void;
  /** Taps below this zoom level zoom in instead of picking (precise placement). */
  minPickZoom?: number;
  onNeedZoom?: () => void;
  className?: string;
  testId?: string;
}

function markerElement(m: MapMarker): HTMLElement {
  const el = document.createElement("div");
  el.className = "map-pin";
  el.style.setProperty("--pin", m.color);
  el.title = m.title ?? m.label ?? "";
  if (m.label) {
    const label = document.createElement("span");
    label.className = "map-pin-label";
    label.textContent = m.label;
    el.appendChild(label);
  }
  return el;
}

const MapView = forwardRef<MapViewHandle, Props>(function MapView(
  {
    markers = [],
    lines = [],
    fitTo,
    initialCenter = [10, 30],
    initialZoom = 1.2,
    onPick,
    minPickZoom,
    onNeedZoom,
    className,
    testId,
  },
  ref,
) {
  const container = useRef<HTMLDivElement>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const markerRefs = useRef<maplibregl.Marker[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pickRef = useRef({ onPick, minPickZoom, onNeedZoom });
  pickRef.current = { onPick, minPickZoom, onNeedZoom };

  useImperativeHandle(ref, () => ({
    flyTo: (lng, lat, zoom) => mapRef.current?.flyTo({ center: [lng, lat], zoom: zoom ?? 14, duration: 900 }),
    getZoom: () => mapRef.current?.getZoom() ?? 0,
  }));

  useEffect(() => {
    let cancelled = false;
    let map: maplibregl.Map | null = null;
    let observer: ResizeObserver | null = null;
    englishMapStyle().then((style) => {
      if (cancelled || !container.current) return;
      try {
        map = new maplibregl.Map({
          container: container.current,
          style,
          center: initialCenter,
          zoom: initialZoom,
          attributionControl: { compact: true },
          dragRotate: false,
          pitchWithRotate: false,
          renderWorldCopies: true,
        });
      } catch (e) {
        setError("Your browser can't display the map (WebGL is disabled).");
        return;
      }
      map.touchZoomRotate.disableRotation();
      map.addControl(new maplibregl.NavigationControl({ showCompass: false }), "top-right");
      map.getCanvas().style.cursor = pickRef.current.onPick ? "crosshair" : "";
      map.on("click", (e) => {
        const { onPick, minPickZoom, onNeedZoom } = pickRef.current;
        if (!onPick || !map) return;
        if (minPickZoom !== undefined && map.getZoom() < minPickZoom - 0.01) {
          map.flyTo({ center: e.lngLat, zoom: Math.min(map.getZoom() + 4, minPickZoom), duration: 700 });
          onNeedZoom?.();
          return;
        }
        const wrapped = e.lngLat.wrap();
        onPick(wrapped.lng, wrapped.lat);
      });
      map.on("load", () => {
        if (!map) return;
        map.addSource("lines", { type: "geojson", data: { type: "FeatureCollection", features: [] } });
        map.addLayer({
          id: "lines",
          type: "line",
          source: "lines",
          paint: { "line-color": "#1d2a3a", "line-width": 2.5, "line-dasharray": [2, 1.5] },
        });
        setLoaded(true);
      });
      mapRef.current = map;
      observer = new ResizeObserver(() => map?.resize());
      observer.observe(container.current);
    });
    return () => {
      cancelled = true;
      observer?.disconnect();
      map?.remove();
      mapRef.current = null;
    };
    // The map is created once; props are synced by the effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Markers
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    markerRefs.current.forEach((m) => m.remove());
    markerRefs.current = markers.map((m) =>
      new maplibregl.Marker({ element: markerElement(m), anchor: "bottom" }).setLngLat([m.lng, m.lat]).addTo(map),
    );
  }, [markers, loaded]);

  // Lines
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !loaded) return;
    const source = map.getSource("lines") as maplibregl.GeoJSONSource | undefined;
    source?.setData({
      type: "FeatureCollection",
      features: lines.map((coords) => ({
        type: "Feature",
        properties: {},
        geometry: { type: "LineString", coordinates: coords },
      })),
    });
  }, [lines, loaded]);

  // Fit bounds
  const fitKey = fitTo ? JSON.stringify(fitTo) : "";
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !fitTo || fitTo.length === 0) return;
    if (fitTo.length === 1) {
      map.flyTo({ center: fitTo[0], zoom: Math.max(map.getZoom(), 12), duration: 800 });
      return;
    }
    const [sw, ne] = boundsOf(fitTo);
    map.fitBounds([sw, ne], { padding: 60, maxZoom: 15, duration: 900 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fitKey, loaded]);

  return (
    <div className={`map-view ${className ?? ""}`} data-testid={testId} data-loaded={loaded ? "1" : "0"}>
      <div ref={container} className="map-canvas" />
      {error && <div className="map-error">{error}</div>}
    </div>
  );
});

export default MapView;
