import type { StyleSpecification } from "maplibre-gl";

// OpenFreeMap: free OpenStreetMap vector tiles, no API key.
const STYLE_URL = "https://tiles.openfreemap.org/styles/liberty";

// English first, then the romanized name, then the local name.
const ENGLISH_LABEL = ["coalesce", ["get", "name:en"], ["get", "name:latin"], ["get", "name"]];

let cached: Promise<StyleSpecification | string> | null = null;

/** Rewrite every label that displays a place name so the whole map reads in English. */
function englishify(style: StyleSpecification): StyleSpecification {
  for (const layer of style.layers) {
    if (layer.type !== "symbol" || !layer.layout) continue;
    const field = (layer.layout as Record<string, unknown>)["text-field"];
    if (field && JSON.stringify(field).includes('"name')) {
      (layer.layout as Record<string, unknown>)["text-field"] = ENGLISH_LABEL;
    }
  }
  return style;
}

/** A fresh copy per map: MapLibre may mutate the style object it is given. */
export async function englishMapStyle(): Promise<StyleSpecification | string> {
  const style = await loadStyle();
  return typeof style === "string" ? style : structuredClone(style);
}

function loadStyle(): Promise<StyleSpecification | string> {
  if (!cached) {
    cached = fetch(STYLE_URL)
      .then((r) => {
        if (!r.ok) throw new Error(`style ${r.status}`);
        return r.json() as Promise<StyleSpecification>;
      })
      .then(englishify)
      .catch(() => {
        cached = null;
        // Fall back to the raw style URL (labels may be in local languages).
        return STYLE_URL;
      });
  }
  return cached;
}
