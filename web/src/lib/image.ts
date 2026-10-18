import exifr from "exifr";

const MAX_SIDE = 2560;

export interface PhotoHints {
  year: number | null;
  lat: number | null;
  lng: number | null;
}

/** Year and GPS position from the file's EXIF, used only as suggestions. */
export async function readHints(file: File): Promise<PhotoHints> {
  const hints: PhotoHints = { year: null, lat: null, lng: null };
  try {
    const data = await exifr.parse(file, ["DateTimeOriginal", "CreateDate"]);
    const date: Date | undefined = data?.DateTimeOriginal ?? data?.CreateDate;
    if (date instanceof Date && !isNaN(date.getTime())) hints.year = date.getFullYear();
  } catch {
    /* no EXIF */
  }
  try {
    const gps = await exifr.gps(file);
    if (gps && Number.isFinite(gps.latitude) && Number.isFinite(gps.longitude)) {
      hints.lat = gps.latitude;
      hints.lng = gps.longitude;
    }
  } catch {
    /* no GPS */
  }
  return hints;
}

function isHeic(file: File) {
  return /hei[cf]$/i.test(file.name) || /image\/hei[cf]/i.test(file.type);
}

async function decode(file: Blob): Promise<ImageBitmap> {
  return createImageBitmap(file, { imageOrientation: "from-image" });
}

/**
 * Downscale (≤2560px) and re-encode as JPEG in the browser: faster uploads on mobile
 * networks, EXIF orientation applied. HEIC files are converted when the browser can't
 * decode them natively.
 */
export async function prepareImage(file: File): Promise<Blob> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await decode(file);
  } catch {
    if (!isHeic(file)) return file; // let the server decide
    const { default: heic2any } = await import("heic2any");
    const converted = await heic2any({ blob: file, toType: "image/jpeg", quality: 0.9 });
    const blob = Array.isArray(converted) ? converted[0] : converted;
    bitmap = await decode(blob);
  }
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  const ctx = canvas.getContext("2d");
  if (!ctx) return file;
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.9));
  return blob ?? file;
}
