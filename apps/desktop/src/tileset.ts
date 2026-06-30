import { type MapRender, readProjectImage } from "./api";

/**
 * RPG Maker MV/MZ tile rendering (Wave 2, R1).
 *
 * Flat sheets (A5 / B / C / D / E) are drawn exactly. Autotiles (A1–A4) are
 * drawn with the "fully-surrounded" centre variant (shape 0) — real texture per
 * tile, but WITHOUT the 47-blob neighbour blending, so edges between two
 * autotiles show hard seams. True autotile assembly is the deferred R2 step.
 */

/** Native source tile size in RPG Maker (px). */
const SRC = 48;
/** Tileset image slots, in `tilesetNames` order. */
const SLOTS = 9; // A1,A2,A3,A4,A5,B,C,D,E
/** Cap on the composited map bitmap's largest side (memory bound). */
const MAX_DIM = 4096;

/** Centre (shape-0) quarter offsets for floor-type autotiles (A1, A2, A4 tops). */
const FLOOR_CENTER: ReadonlyArray<readonly [number, number]> = [
  [2, 4],
  [1, 4],
  [2, 3],
  [1, 3],
];
/** Centre (shape-0) quarter offsets for wall-type autotiles (A3, A4 walls). */
const WALL_CENTER: ReadonlyArray<readonly [number, number]> = [
  [2, 2],
  [1, 2],
  [2, 1],
  [1, 1],
];

const isA1 = (id: number) => id >= 2048 && id < 2816;
const isA2 = (id: number) => id >= 2816 && id < 4352;
const isA3 = (id: number) => id >= 4352 && id < 5888;
const isA5 = (id: number) => id >= 1536 && id < 2048;
const isAutotile = (id: number) => id >= 2048;

type Bitmaps = (ImageBitmap | null)[];

/** Loads the 9 tileset image slots; missing/unreadable slots become `null`. */
export async function loadTileset(path: string, names: string[]): Promise<Bitmaps> {
  const out: Bitmaps = new Array(SLOTS).fill(null);
  await Promise.all(
    names.slice(0, SLOTS).map(async (name, i) => {
      if (!name) return;
      try {
        const buf = await readProjectImage(path, `img/tilesets/${name}.png`);
        out[i] = await createImageBitmap(new Blob([buf]));
      } catch {
        out[i] = null;
      }
    }),
  );
  return out;
}

/** Releases the decoded tileset bitmaps. */
export function disposeTileset(bitmaps: Bitmaps | null): void {
  if (!bitmaps) return;
  for (const b of bitmaps) b?.close();
}

/** Draws a flat (non-autotile) tile: A5 or B/C/D/E. */
function drawNormalTile(
  ctx: CanvasRenderingContext2D,
  bitmaps: Bitmaps,
  id: number,
  dx: number,
  dy: number,
  ts: number,
): void {
  const set = isA5(id) ? 4 : 5 + Math.floor(id / 256);
  const src = bitmaps[set];
  if (!src) return;
  const sx = ((Math.floor(id / 128) % 2) * 8 + (id % 8)) * SRC;
  const sy = (Math.floor((id % 256) / 8) % 16) * SRC;
  ctx.drawImage(src, sx, sy, SRC, SRC, dx, dy, ts, ts);
}

/** Draws an autotile as its centre (shape-0) variant — R1 approximation. */
function drawAutotile(
  ctx: CanvasRenderingContext2D,
  bitmaps: Bitmaps,
  id: number,
  dx: number,
  dy: number,
  ts: number,
): void {
  const kind = Math.floor((id - 2048) / 48);
  const tx = kind % 8;
  const ty = Math.floor(kind / 8);
  let set = 0;
  let bx = 0;
  let by = 0;
  let wall = false;

  if (isA1(id)) {
    set = 0;
    if (kind === 0) {
      bx = 0;
      by = 0;
    } else if (kind === 1) {
      bx = 0;
      by = 3;
    } else if (kind === 2) {
      bx = 6;
      by = 0;
    } else if (kind === 3) {
      bx = 6;
      by = 3;
    } else {
      bx = Math.floor(tx / 4) * 8;
      by = ty * 6 + (Math.floor(tx / 2) % 2) * 3;
      if (kind % 2 !== 0) bx += 6;
    }
  } else if (isA2(id)) {
    set = 1;
    bx = tx * 2;
    by = (ty - 2) * 3;
  } else if (isA3(id)) {
    set = 2;
    bx = tx * 2;
    by = (ty - 6) * 2;
    wall = true;
  } else {
    // A4 — even rows are floor tops, odd rows are walls.
    set = 3;
    bx = tx * 2;
    if (ty % 2 === 0) {
      by = (ty - 10) * 3;
    } else {
      by = (ty - 11) * 3 + 0.5;
      wall = true;
    }
  }

  const src = bitmaps[set];
  if (!src) return;
  const quads = wall ? WALL_CENTER : FLOOR_CENTER;
  const half = ts / 2;
  for (let i = 0; i < 4; i++) {
    const [qsx, qsy] = quads[i];
    const sx = (bx * 2 + qsx) * (SRC / 2);
    const sy = (by * 2 + qsy) * (SRC / 2);
    const ddx = dx + (i % 2) * half;
    const ddy = dy + Math.floor(i / 2) * half;
    ctx.drawImage(src, sx, sy, SRC / 2, SRC / 2, ddx, ddy, half, half);
  }
}

/** Draws a single tile (dispatches flat vs autotile). */
function drawTile(
  ctx: CanvasRenderingContext2D,
  bitmaps: Bitmaps,
  id: number,
  dx: number,
  dy: number,
  ts: number,
): void {
  if (id <= 0) return;
  if (isAutotile(id)) drawAutotile(ctx, bitmaps, id, dx, dy, ts);
  else drawNormalTile(ctx, bitmaps, id, dx, dy, ts);
}

/** A composited map bitmap plus the tile size it was rendered at. */
export interface MapTiles {
  canvas: HTMLCanvasElement;
  width: number;
  height: number;
}

/**
 * Composites all tile layers (0..3, skipping shadow/region) of a map into one
 * offscreen canvas. The tile size is capped so the canvas stays within
 * {@link MAX_DIM}; the canvas is later blitted (scaled) under the events.
 */
export function composeMap(render: MapRender, bitmaps: Bitmaps): MapTiles | null {
  const cols = render.width;
  const rows = render.height;
  if (cols <= 0 || rows <= 0) return null;
  if (!bitmaps.some(Boolean)) return null; // no images decoded → nothing to draw

  const tilePx = Math.max(6, Math.min(SRC, Math.floor(MAX_DIM / Math.max(cols, rows))));
  const canvas = document.createElement("canvas");
  canvas.width = cols * tilePx;
  canvas.height = rows * tilePx;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.imageSmoothingEnabled = false;

  const plane = cols * rows;
  for (let z = 0; z < 4; z++) {
    const base = z * plane;
    for (let y = 0; y < rows; y++) {
      for (let x = 0; x < cols; x++) {
        const id = render.data[base + y * cols + x] | 0;
        if (id > 0) drawTile(ctx, bitmaps, id, x * tilePx, y * tilePx, tilePx);
      }
    }
  }
  return { canvas, width: canvas.width, height: canvas.height };
}

/** Deterministic, theme-independent color for a region id (1..255). */
function regionColor(id: number): string {
  const hue = (id * 47) % 360;
  return `hsl(${hue}, 70%, 50%)`;
}

/**
 * Composites the region layer (data plane 5) into one offscreen canvas: each
 * non-zero region id paints a solid, deterministically-colored cell. Returns
 * `null` when the map declares no regions. The caller blits it (translucent)
 * over the tiles as a heat overlay.
 */
export function composeRegions(render: MapRender): MapTiles | null {
  const cols = render.width;
  const rows = render.height;
  if (cols <= 0 || rows <= 0) return null;
  const plane = cols * rows;
  const base = 5 * plane;
  if (render.data.length < base + plane) return null; // no region plane present

  const tilePx = Math.max(6, Math.min(SRC, Math.floor(MAX_DIM / Math.max(cols, rows))));
  const canvas = document.createElement("canvas");
  canvas.width = cols * tilePx;
  canvas.height = rows * tilePx;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;

  let any = false;
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const id = render.data[base + y * cols + x] | 0;
      if (id <= 0) continue;
      any = true;
      ctx.fillStyle = regionColor(id);
      ctx.fillRect(x * tilePx, y * tilePx, tilePx, tilePx);
    }
  }
  if (!any) return null;
  return { canvas, width: canvas.width, height: canvas.height };
}

/** Renders a single tile id (e.g. a tile-graphic event) into a 48×48 canvas. */
export function renderTile(bitmaps: Bitmaps, tileId: number): HTMLCanvasElement | null {
  if (tileId <= 0 || !bitmaps.some(Boolean)) return null;
  const canvas = document.createElement("canvas");
  canvas.width = SRC;
  canvas.height = SRC;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.imageSmoothingEnabled = false;
  drawTile(ctx, bitmaps, tileId, 0, 0, SRC);
  return canvas;
}
