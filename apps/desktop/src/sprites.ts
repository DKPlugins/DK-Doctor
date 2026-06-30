import { type MapAtlas, readProjectImage } from "./api";
import { renderTile } from "./tileset";

/** Per-event pre-rendered graphic (character cell or tile), keyed by event id. */
export type EventSprites = Map<number, HTMLCanvasElement>;

/** Loads one character sheet (`img/characters/<name>.png`), or `null` on failure. */
async function loadSheet(path: string, name: string): Promise<ImageBitmap | null> {
  try {
    const buf = await readProjectImage(path, `img/characters/${name}.png`);
    return await createImageBitmap(new Blob([buf]));
  } catch {
    return null;
  }
}

/**
 * Extracts a single character cell from a sheet. A `$`-named sheet is one
 * character (3×4); a normal sheet packs 8 characters (4×2 blocks of 3×4).
 */
function extractCharacter(
  sheet: ImageBitmap,
  name: string,
  index: number,
  direction: number,
  pattern: number,
): HTMLCanvasElement | null {
  const big = name.includes("$");
  const cols = big ? 3 : 12;
  const rows = big ? 4 : 8;
  const cw = sheet.width / cols;
  const ch = sheet.height / rows;
  if (cw <= 0 || ch <= 0) return null;
  const blockCol = big ? 0 : (index % 4) * 3;
  const blockRow = big ? 0 : Math.floor(index / 4) * 4;
  const col = blockCol + Math.min(2, Math.max(0, pattern));
  const dir = direction >= 2 && direction <= 8 ? direction : 2;
  const row = blockRow + (Math.floor(dir / 2) - 1); // down/left/right/up → 0..3
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(cw));
  canvas.height = Math.max(1, Math.round(ch));
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(sheet, col * cw, row * ch, cw, ch, 0, 0, canvas.width, canvas.height);
  return canvas;
}

/**
 * Builds the per-event sprite map for a map: character-graphic events draw from
 * `img/characters`, tile-graphic events from the (already-loaded) tileset.
 * Sheets are loaded once per unique name and released afterwards.
 */
export async function buildEventSprites(
  path: string,
  map: MapAtlas,
  tilesetBitmaps: (ImageBitmap | null)[] | null,
): Promise<EventSprites> {
  const sprites: EventSprites = new Map();
  const names = new Set<string>();
  for (const e of map.events) {
    const g = e.graphic;
    if (g && g.tileId === 0 && g.characterName) names.add(g.characterName);
  }
  const sheets = new Map<string, ImageBitmap | null>();
  await Promise.all(
    [...names].map(async (n) => {
      sheets.set(n, await loadSheet(path, n));
    }),
  );

  for (const e of map.events) {
    const g = e.graphic;
    if (!g) continue;
    if (g.tileId > 0) {
      if (tilesetBitmaps) {
        const cv = renderTile(tilesetBitmaps, g.tileId);
        if (cv) sprites.set(e.id, cv);
      }
    } else if (g.characterName) {
      const sheet = sheets.get(g.characterName);
      if (sheet) {
        const cv = extractCharacter(sheet, g.characterName, g.characterIndex, g.direction, g.pattern);
        if (cv) sprites.set(e.id, cv);
      }
    }
  }

  for (const s of sheets.values()) s?.close();
  return sprites;
}
