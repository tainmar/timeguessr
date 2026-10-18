import { useMemo, useRef, useState, type FormEvent } from "react";
import MapView, { type MapViewHandle } from "./MapView";

export const PRECISE_ZOOM = 13;

interface Props {
  value: { lat: number; lng: number } | null;
  onChange: (v: { lat: number; lng: number }) => void;
  suggestion?: { lat: number; lng: number } | null;
}

interface SearchResult {
  display_name: string;
  lat: string;
  lon: string;
}

/**
 * Search a place (OpenStreetMap Nominatim, English results) or tap the map.
 * Taps on a zoomed-out map zoom in first so the pin always lands precisely.
 */
export default function LocationPicker({ value, onChange, suggestion }: Props) {
  const map = useRef<MapViewHandle>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  const search = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!query.trim()) return;
    setSearching(true);
    setHint(null);
    try {
      const url = `https://nominatim.openstreetmap.org/search?format=jsonv2&limit=6&accept-language=en&q=${encodeURIComponent(query)}`;
      const res = await fetch(url, { headers: { Accept: "application/json" } });
      const data = (await res.json()) as SearchResult[];
      setResults(data);
      if (data.length === 0) setHint("No place found. Try another spelling, or tap on the map.");
    } catch {
      setHint("Search is unavailable right now: tap on the map instead.");
    } finally {
      setSearching(false);
    }
  };

  const markers = useMemo(
    () => (value ? [{ id: "pick", lat: value.lat, lng: value.lng, color: "#e8a33d", title: "Photo location" }] : []),
    [value],
  );

  return (
    <div className="location-picker">
      <div className="search-row" role="search">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") search();
          }}
          placeholder="Search a place: Eiffel Tower, Lisbon, 10 Downing Street…"
          aria-label="Search a place"
          data-testid="place-search"
        />
        <button type="button" className="btn" onClick={() => search()} disabled={searching}>
          {searching ? "…" : "Search"}
        </button>
      </div>
      {results.length > 0 && (
        <ul className="search-results">
          {results.map((r) => (
            <li key={`${r.lat},${r.lon},${r.display_name}`}>
              <button
                type="button"
                onClick={() => {
                  const lat = Number(r.lat);
                  const lng = Number(r.lon);
                  map.current?.flyTo(lng, lat, 16);
                  onChange({ lat, lng });
                  setResults([]);
                  setHint("Pin placed. Tap the map to adjust it to the exact spot.");
                }}
              >
                {r.display_name}
              </button>
            </li>
          ))}
        </ul>
      )}
      {suggestion && (!value || value.lat !== suggestion.lat || value.lng !== suggestion.lng) && (
        <button
          type="button"
          className="suggestion"
          onClick={() => {
            onChange(suggestion);
            map.current?.flyTo(suggestion.lng, suggestion.lat, 16);
          }}
        >
          📍 Use the location stored in the photo
        </button>
      )}
      <MapView
        ref={map}
        className="picker-map"
        testId="picker-map"
        markers={markers}
        initialCenter={value ? [value.lng, value.lat] : undefined}
        initialZoom={value ? 15 : undefined}
        minPickZoom={PRECISE_ZOOM}
        onNeedZoom={() => setHint("Zooming in… tap again on the exact spot.")}
        onPick={(lng, lat) => {
          onChange({ lat, lng });
          setHint(null);
        }}
      />
      <p className="hint">
        {hint ??
          (value
            ? `Pin at ${value.lat.toFixed(5)}, ${value.lng.toFixed(5)}. Tap elsewhere to move it.`
            : "Tap the map to zoom in, then tap the exact spot where the photo was taken.")}
      </p>
    </div>
  );
}
