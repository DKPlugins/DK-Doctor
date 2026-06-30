import type { MapAtlas, Report, Severity } from "./api";
import type { MapTiles } from "./tileset";
import type { EventSprites } from "./sprites";

/**
 * Interactive schematic map canvas: draws the map grid, event boxes (colored by
 * their worst finding), and a problem-count "cloud" badge per problematic event.
 * Pan (drag), zoom (wheel), and click-to-select an event. Real tile art (when
 * available) layers under the events; an optional region heat overlay and a
 * minimap for large maps draw on top.
 */
export interface CanvasOpts {
  map: MapAtlas;
  /** eventId → finding indices on this map's event. */
  eventFindings: Map<number, number[]>;
  report: Report;
  /** Called when an event is clicked (null = empty tile / deselect). */
  onSelect: (eventId: number | null) => void;
  /** Event ids carrying ≥1 finding new since the previous run (dashed ring). */
  newEvents?: Set<number> | null;
  /** Initial viewport to restore; when absent, the map is auto-fit. */
  initView?: { ox: number; oy: number; cell: number } | null;
  /** Start with the region heat overlay on. */
  regionsOn?: boolean;
  /** Notified (debounced) when the viewport changes, for persistence. */
  onView?: (view: { ox: number; oy: number; cell: number }) => void;
}

/** Imperative handle over a mounted canvas. */
export interface AtlasController {
  select(eventId: number | null): void;
  /** Selects an event and recenters the viewport on it (for "show on map"). */
  focus(eventId: number): void;
  fit(): void;
  zoom(factor: number): void;
  /** Attaches (or clears) the composited tile layer drawn under the events. */
  setTiles(tiles: MapTiles | null): void;
  /** Attaches (or clears) the per-event sprite graphics drawn on the grid. */
  setSprites(sprites: EventSprites | null): void;
  /** Attaches (or clears) the region heat overlay drawn over the tiles. */
  setRegions(regions: MapTiles | null): void;
  /** Toggles the region heat overlay (or sets it explicitly); returns the new state. */
  toggleRegions(on?: boolean): boolean;
  /** Recenters the viewport on the worst-severity event (no-op if none). */
  goToWorst(): void;
  /** Current viewport (pan offset + tile size). */
  getView(): { ox: number; oy: number; cell: number };
  /** Renders the whole annotated map to a PNG data URL. */
  toDataURL(): string;
  destroy(): void;
}

const MIN_CELL = 3;
const MAX_CELL = 64;
/** A map this large (cols/rows) gets the minimap overview. */
const BIG_COLS = 80;
const BIG_ROWS = 90;
/** Largest side of the exported PNG (memory bound). */
const EXPORT_CAP = 8192;
/** Dashed ring color for events new since the previous run (non-theme accent). */
const NEW_RING = "rgba(245,197,66,0.95)";

/** Severity → rank (lower = worse) for picking a cluster's worst color. */
const SEV_RANK: Record<Severity, number> = { error: 0, warning: 1, info: 2 };

/** Resolves a CSS color expression (e.g. `var(--sev-error)`) to `rgb(...)`. */
function makeResolver(): { resolve(c: string): string; dispose(): void } {
  const probe = document.createElement("span");
  probe.style.cssText = "position:absolute;left:-9999px;width:0;height:0";
  document.body.appendChild(probe);
  return {
    resolve(c: string): string {
      probe.style.color = "";
      probe.style.color = c;
      return getComputedStyle(probe).color || "rgb(136,136,136)";
    },
    dispose() {
      probe.remove();
    },
  };
}

/** `rgb(r,g,b)` → `rgba(r,g,b,a)`. */
function withAlpha(rgb: string, a: number): string {
  const m = rgb.match(/[\d.]+/g);
  if (!m || m.length < 3) return rgb;
  return `rgba(${m[0]}, ${m[1]}, ${m[2]}, ${a})`;
}

/** A problem badge to draw, with its severity rank for clustering. */
interface Cloud {
  x: number;
  y: number;
  n: number;
  color: string;
  rank: number;
}

/** Geometry of a render pass (on-screen viewport or full-map export). */
interface Scene {
  ox: number;
  oy: number;
  cell: number;
  w: number;
  h: number;
  /** Cull events outside the viewport (on-screen) vs draw all (export). */
  cull: boolean;
  /** Draw the selection outline (on-screen only). */
  selectable: boolean;
}

export function mountCanvas(canvas: HTMLCanvasElement, opts: CanvasOpts): AtlasController {
  const ctx = canvas.getContext("2d")!;
  const { map, report, eventFindings } = opts;
  const newEvents = opts.newEvents ?? null;

  // Effective grid bounds: tolerate missing width/height by covering events.
  let cols = map.width;
  let rows = map.height;
  for (const e of map.events) {
    cols = Math.max(cols, e.x + 1);
    rows = Math.max(rows, e.y + 1);
  }
  cols = Math.max(1, cols);
  rows = Math.max(1, rows);

  // Tile → topmost event id (later events win, as in the editor stacking).
  const tileEvent = new Map<string, number>();
  for (const e of map.events) tileEvent.set(`${e.x},${e.y}`, e.id);

  const r = makeResolver();
  const col = {
    error: r.resolve("var(--sev-error)"),
    warning: r.resolve("var(--sev-warning)"),
    info: r.resolve("var(--sev-info)"),
    ok: r.resolve("var(--sev-ok)"),
    grid: r.resolve("var(--border)"),
    bg: r.resolve("var(--surface)"),
    text: r.resolve("var(--text)"),
  };
  r.dispose();
  const sevColor = (s: Severity) =>
    s === "error" ? col.error : s === "warning" ? col.warning : col.info;

  let cell = 24;
  let ox = 0;
  let oy = 0;
  let selected: number | null = null;
  let raf = 0;
  let tiles: MapTiles | null = null;
  let sprites: EventSprites | null = null;
  let regions: MapTiles | null = null;
  let heatOn = !!opts.regionsOn;
  let didInit = false;
  let saveTimer = 0;
  /** Screen-space rect of the minimap, for click-to-pan (null when hidden). */
  let minimap: { mx: number; my: number; mw: number; mh: number } | null = null;

  const clampCell = (c: number) => Math.max(MIN_CELL, Math.min(MAX_CELL, c));

  function applySelect(id: number | null): void {
    selected = id;
    schedule();
  }

  function size(): { w: number; h: number } {
    const rect = canvas.getBoundingClientRect();
    return { w: Math.max(1, rect.width), h: Math.max(1, rect.height) };
  }

  function setupSize(): void {
    const dpr = window.devicePixelRatio || 1;
    const { w, h } = size();
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function fit(): void {
    const { w, h } = size();
    cell = Math.max(MIN_CELL, Math.min(MAX_CELL, Math.floor(Math.min(w / cols, h / rows))));
    if (!isFinite(cell) || cell < MIN_CELL) cell = MIN_CELL;
    ox = Math.round((w - cols * cell) / 2);
    oy = Math.round((h - rows * cell) / 2);
  }

  /** Clamps the pan offset so at least a margin of the field stays on-screen. */
  function clampOffset(): void {
    const { w, h } = size();
    const fieldW = cols * cell;
    const fieldH = rows * cell;
    const margin = Math.min(80, fieldW, fieldH);
    ox = Math.min(w - margin, Math.max(margin - fieldW, ox));
    oy = Math.min(h - margin, Math.max(margin - fieldH, oy));
  }

  /** First-time init: restore a saved viewport or auto-fit, once real dims exist. */
  function init(): void {
    if (didInit) return;
    const { w, h } = size();
    if (w <= 1 || h <= 1) return; // wait for layout (ResizeObserver retries)
    didInit = true;
    const iv = opts.initView;
    if (iv && isFinite(iv.ox) && isFinite(iv.oy) && isFinite(iv.cell)) {
      cell = clampCell(iv.cell);
      ox = iv.ox;
      oy = iv.oy;
      clampOffset(); // a viewport saved at a different window size may be off-screen
    } else {
      fit();
    }
  }

  function worst(indices: number[] | undefined): Severity | null {
    if (!indices || indices.length === 0) return null;
    let wsev: Severity | null = null;
    for (const i of indices) {
      const s = report.findings[i].severity;
      if (s === "error") return "error";
      if (s === "warning") wsev = "warning";
      else if (s === "info" && wsev === null) wsev = "info";
    }
    return wsev;
  }

  function schedule(): void {
    if (!raf) raf = requestAnimationFrame(() => ((raf = 0), draw()));
  }

  /** Debounced notify-on-viewport-change for persistence. */
  function saveView(): void {
    if (!opts.onView) return;
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      saveTimer = 0;
      opts.onView?.({ ox, oy, cell });
    }, 250);
  }

  function badge(c: CanvasRenderingContext2D, x: number, y: number, n: number, color: string, cs: number): void {
    // Larger and high-contrast so it never blends into the tile art below:
    // a soft drop shadow, a solid severity-coloured disc, and a white ring.
    const rad = Math.max(10, Math.min(15, cs * 0.42));
    const cx = x - rad * 0.5;
    const cy = y + rad * 0.5;
    c.save();
    c.shadowColor = "rgba(0,0,0,0.5)";
    c.shadowBlur = 4;
    c.shadowOffsetY = 1;
    c.beginPath();
    c.arc(cx, cy, rad, 0, Math.PI * 2);
    c.fillStyle = color;
    c.fill();
    c.restore();
    c.beginPath();
    c.arc(cx, cy, rad, 0, Math.PI * 2);
    c.strokeStyle = "#fff";
    c.lineWidth = 2;
    c.stroke();
    c.fillStyle = "#fff";
    c.font = `700 ${Math.round(rad * 1.15)}px ui-sans-serif, system-ui, sans-serif`;
    c.textAlign = "center";
    c.textBaseline = "middle";
    c.fillText(n > 9 ? "9+" : String(n), cx, cy + 0.5);
    c.textAlign = "left";
    c.textBaseline = "alphabetic";
  }

  /** Collapses overlapping count badges into one (summed count, worst color). */
  function clusterClouds(cl: Cloud[], cs: number): Cloud[] {
    const thr = Math.max(cs * 0.9, 10);
    const used = new Array(cl.length).fill(false);
    const out: Cloud[] = [];
    for (let i = 0; i < cl.length; i++) {
      if (used[i]) continue;
      let { n, color, rank } = cl[i];
      const { x, y } = cl[i];
      for (let j = i + 1; j < cl.length; j++) {
        if (used[j]) continue;
        if (Math.abs(cl[j].x - x) < thr && Math.abs(cl[j].y - y) < thr) {
          used[j] = true;
          n += cl[j].n;
          if (cl[j].rank < rank) {
            rank = cl[j].rank;
            color = cl[j].color;
          }
        }
      }
      out.push({ x, y, n, color, rank });
    }
    return out;
  }

  function selOutline(c: CanvasRenderingContext2D, ex: number, ey: number, cs: number): void {
    c.strokeStyle = col.text;
    c.lineWidth = 2;
    c.strokeRect(ex + 1, ey + 1, Math.max(0, cs - 2), Math.max(0, cs - 2));
  }

  /** Paints the field (tiles/regions/grid/events/badges) for a given scene. */
  function paint(c: CanvasRenderingContext2D, sc: Scene): void {
    const fieldW = cols * sc.cell;
    const fieldH = rows * sc.cell;

    // Map field: real tiles when available, else a flat schematic fill.
    if (tiles) {
      c.imageSmoothingEnabled = false;
      c.drawImage(tiles.canvas, 0, 0, tiles.width, tiles.height, sc.ox, sc.oy, fieldW, fieldH);
    } else {
      c.fillStyle = col.bg;
      c.fillRect(sc.ox, sc.oy, fieldW, fieldH);
    }

    // Region heat overlay (layer 5) — over tiles, under events.
    if (heatOn && regions) {
      c.save();
      c.globalAlpha = 0.45;
      c.imageSmoothingEnabled = false;
      c.drawImage(regions.canvas, 0, 0, regions.width, regions.height, sc.ox, sc.oy, fieldW, fieldH);
      c.restore();
    }

    // Grid — lighter over tiles, skipped when cells get too small.
    if (!tiles || sc.cell >= 12) {
      const step = sc.cell < 7 ? 8 : 1;
      c.strokeStyle = tiles ? withAlpha(col.grid, 0.22) : col.grid;
      c.lineWidth = 1;
      c.beginPath();
      for (let x = 0; x <= cols; x += step) {
        const px = Math.round(sc.ox + x * sc.cell) + 0.5;
        c.moveTo(px, sc.oy);
        c.lineTo(px, sc.oy + fieldH);
      }
      for (let y = 0; y <= rows; y += step) {
        const py = Math.round(sc.oy + y * sc.cell) + 0.5;
        c.moveTo(sc.ox, py);
        c.lineTo(sc.ox + fieldW, py);
      }
      c.stroke();
    }

    // Events — pass 1: sprites + boxes + new-rings. Count clouds are deferred to
    // pass 2 so a neighbouring event's sprite/box can never paint over a badge.
    const clouds: Cloud[] = [];
    for (const e of map.events) {
      const ex = sc.ox + e.x * sc.cell;
      const ey = sc.oy + e.y * sc.cell;
      if (sc.cull && (ex + sc.cell < 0 || ey + sc.cell < 0 || ex > sc.w || ey > sc.h)) continue;
      const indices = eventFindings.get(e.id);
      const ws = worst(indices);
      const isSel = sc.selectable && e.id === selected;
      const isNew = !!newEvents && newEvents.has(e.id);
      const spr = sprites?.get(e.id) ?? null;

      // Real event graphic — anchored to the tile bottom, aspect-preserving.
      if (spr && sc.cell >= 4) {
        const dw = sc.cell;
        const dh = sc.cell * (spr.height / spr.width);
        c.drawImage(spr, ex, ey + sc.cell - dh, dw, dh);
      }

      if (!ws) {
        // Clean event: a faint marker only on the blank schematic grid.
        if (!spr && !tiles) {
          c.fillStyle = withAlpha(col.ok, 0.12);
          c.fillRect(ex + 1, ey + 1, Math.max(0, sc.cell - 2), Math.max(0, sc.cell - 2));
          c.strokeStyle = withAlpha(col.ok, 0.5);
          c.lineWidth = 1;
          c.strokeRect(ex + 1.5, ey + 1.5, Math.max(0, sc.cell - 3), Math.max(0, sc.cell - 3));
        }
        if (isNew) newRing(c, ex, ey, sc.cell);
        if (isSel) selOutline(c, ex, ey, sc.cell);
        continue;
      }

      // Problem event: a tinted box + border over whatever is beneath.
      const base = sevColor(ws);
      c.fillStyle = withAlpha(base, spr ? 0.42 : tiles ? 0.5 : 0.32);
      c.fillRect(ex + 1, ey + 1, Math.max(0, sc.cell - 2), Math.max(0, sc.cell - 2));
      c.strokeStyle = withAlpha(base, 0.95);
      c.lineWidth = 1;
      c.strokeRect(ex + 1.5, ey + 1.5, Math.max(0, sc.cell - 3), Math.max(0, sc.cell - 3));
      if (isNew) newRing(c, ex, ey, sc.cell);
      if (isSel) selOutline(c, ex, ey, sc.cell);
      if (indices && sc.cell >= 6) {
        clouds.push({ x: ex + sc.cell, y: ey, n: indices.length, color: base, rank: SEV_RANK[ws] });
      }
    }

    // Pass 2: clustered count clouds on top of everything.
    for (const cc of clusterClouds(clouds, sc.cell)) badge(c, cc.x, cc.y, cc.n, cc.color, sc.cell);
  }

  /** Dashed ring marking an event with findings new since the previous run. */
  function newRing(c: CanvasRenderingContext2D, ex: number, ey: number, cs: number): void {
    if (cs < 5) return;
    c.save();
    c.strokeStyle = NEW_RING;
    c.lineWidth = 2;
    c.setLineDash([3, 2]);
    c.strokeRect(ex - 1.5, ey - 1.5, cs + 3, cs + 3);
    c.restore();
  }

  /** Draws the minimap overview + viewport frame (large maps only). */
  function drawMinimap(w: number, h: number): void {
    minimap = null;
    if (cols < BIG_COLS && rows < BIG_ROWS) return;
    const pad = 8;
    const mw = Math.min(160, Math.round(w * 0.28));
    const mh = Math.max(36, Math.round((mw * rows) / cols));
    const mx = w - mw - pad;
    const my = h - mh - pad;
    ctx.save();
    ctx.globalAlpha = 0.9;
    ctx.fillStyle = col.bg;
    ctx.fillRect(mx, my, mw, mh);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = col.grid;
    ctx.lineWidth = 1;
    ctx.strokeRect(mx + 0.5, my + 0.5, mw, mh);
    const dw = Math.max(1.5, mw / cols);
    const dh = Math.max(1.5, mh / rows);
    for (const e of map.events) {
      const ws = worst(eventFindings.get(e.id));
      if (!ws) continue;
      ctx.fillStyle = sevColor(ws);
      ctx.fillRect(mx + (e.x / cols) * mw, my + (e.y / rows) * mh, dw, dh);
    }
    // Viewport frame: world units visible = [-ox/cell .. (w-ox)/cell].
    const vx = Math.max(0, -ox / cell / cols);
    const vy = Math.max(0, -oy / cell / rows);
    const vw = Math.min(1 - vx, w / cell / cols);
    const vh = Math.min(1 - vy, h / cell / rows);
    ctx.strokeStyle = col.text;
    ctx.lineWidth = 1.5;
    ctx.strokeRect(mx + vx * mw + 0.5, my + vy * mh + 0.5, Math.max(2, vw * mw), Math.max(2, vh * mh));
    ctx.restore();
    minimap = { mx, my, mw, mh };
  }

  function draw(): void {
    const { w, h } = size();
    ctx.clearRect(0, 0, w, h);
    paint(ctx, { ox, oy, cell, w, h, cull: true, selectable: true });
    drawMinimap(w, h);
  }

  /** Recenters the viewport so world tile (tx,ty) sits at the screen center. */
  function centerOn(tx: number, ty: number): void {
    const { w, h } = size();
    ox = w / 2 - (tx + 0.5) * cell;
    oy = h / 2 - (ty + 0.5) * cell;
    schedule();
    saveView();
  }

  /** Worst-severity event (rank, then count), or null if no problem events. */
  function worstEvent(): { x: number; y: number } | null {
    let best: { x: number; y: number; rank: number; n: number } | null = null;
    for (const e of map.events) {
      const indices = eventFindings.get(e.id);
      const ws = worst(indices);
      if (!ws) continue;
      const rank = SEV_RANK[ws];
      const n = indices ? indices.length : 0;
      if (!best || rank < best.rank || (rank === best.rank && n > best.n)) {
        best = { x: e.x, y: e.y, rank, n };
      }
    }
    return best ? { x: best.x, y: best.y } : null;
  }

  // --- interaction ---------------------------------------------------------
  let dragging = false;
  let moved = false;
  let lastX = 0;
  let lastY = 0;

  function hit(clientX: number, clientY: number): number | null {
    const rect = canvas.getBoundingClientRect();
    const tx = Math.floor((clientX - rect.left - ox) / cell);
    const ty = Math.floor((clientY - rect.top - oy) / cell);
    return tileEvent.get(`${tx},${ty}`) ?? null;
  }

  /** Click inside the minimap → recenter on that point; returns true if handled. */
  function minimapClick(lx: number, ly: number): boolean {
    if (!minimap) return false;
    const { mx, my, mw, mh } = minimap;
    if (lx < mx || lx > mx + mw || ly < my || ly > my + mh) return false;
    centerOn(((lx - mx) / mw) * cols - 0.5, ((ly - my) / mh) * rows - 0.5);
    return true;
  }

  function onDown(e: PointerEvent): void {
    dragging = true;
    moved = false;
    lastX = e.clientX;
    lastY = e.clientY;
    canvas.setPointerCapture(e.pointerId);
  }
  function onMove(e: PointerEvent): void {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    if (Math.abs(dx) + Math.abs(dy) > 3) moved = true;
    ox += dx;
    oy += dy;
    lastX = e.clientX;
    lastY = e.clientY;
    schedule();
  }
  function onUp(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    try {
      canvas.releasePointerCapture(e.pointerId);
    } catch {
      /* pointer already released */
    }
    if (moved) {
      saveView();
      return;
    }
    const rect = canvas.getBoundingClientRect();
    if (minimapClick(e.clientX - rect.left, e.clientY - rect.top)) return;
    const id = hit(e.clientX, e.clientY);
    applySelect(id);
    opts.onSelect(id);
  }
  function zoomAt(px: number, py: number, factor: number): void {
    const next = Math.max(MIN_CELL, Math.min(MAX_CELL, cell * factor));
    if (next === cell) return;
    const wx = (px - ox) / cell;
    const wy = (py - oy) / cell;
    ox = px - wx * next;
    oy = py - wy * next;
    cell = next;
    schedule();
    saveView();
  }
  function onWheel(e: WheelEvent): void {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    zoomAt(e.clientX - rect.left, e.clientY - rect.top, e.deltaY < 0 ? 1.15 : 1 / 1.15);
  }

  canvas.addEventListener("pointerdown", onDown);
  canvas.addEventListener("pointermove", onMove);
  canvas.addEventListener("pointerup", onUp);
  canvas.addEventListener("pointercancel", onUp);
  canvas.addEventListener("wheel", onWheel, { passive: false });

  const ro = new ResizeObserver(() => {
    setupSize();
    init();
    draw();
  });
  ro.observe(canvas);

  // Initial paint (ResizeObserver also fires once, but paint now for instant feedback).
  setupSize();
  init();
  draw();

  /** Renders the whole annotated map (not just the viewport) to a PNG data URL. */
  function exportPng(): string {
    let ec = tiles ? Math.max(8, Math.min(48, Math.round(tiles.width / cols))) : 24;
    if (cols * ec > EXPORT_CAP) ec = Math.floor(EXPORT_CAP / cols);
    if (rows * ec > EXPORT_CAP) ec = Math.min(ec, Math.floor(EXPORT_CAP / rows));
    ec = Math.max(2, ec);
    const off = document.createElement("canvas");
    off.width = cols * ec;
    off.height = rows * ec;
    const c = off.getContext("2d");
    if (!c) return canvas.toDataURL("image/png");
    c.fillStyle = col.bg;
    c.fillRect(0, 0, off.width, off.height);
    paint(c, { ox: 0, oy: 0, cell: ec, w: off.width, h: off.height, cull: false, selectable: false });
    return off.toDataURL("image/png");
  }

  const api: AtlasController = {
    select(id) {
      applySelect(id);
    },
    focus(id) {
      applySelect(id);
      const e = map.events.find((ev) => ev.id === id);
      if (e) centerOn(e.x, e.y);
    },
    fit() {
      fit();
      schedule();
      saveView();
    },
    zoom(factor) {
      const { w, h } = size();
      zoomAt(w / 2, h / 2, factor);
    },
    setTiles(t) {
      tiles = t;
      schedule();
    },
    setSprites(s) {
      sprites = s;
      schedule();
    },
    setRegions(rg) {
      regions = rg;
      schedule();
    },
    toggleRegions(on) {
      heatOn = on === undefined ? !heatOn : on;
      schedule();
      return heatOn;
    },
    goToWorst() {
      const w = worstEvent();
      if (w) centerOn(w.x, w.y);
    },
    getView() {
      return { ox, oy, cell };
    },
    toDataURL() {
      return exportPng();
    },
    destroy() {
      if (raf) cancelAnimationFrame(raf);
      // Flush a pending debounced viewport save so a pan/zoom made just before
      // teardown (map switch / tab switch) is not silently lost.
      if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = 0;
        opts.onView?.({ ox, oy, cell });
      }
      ro.disconnect();
      canvas.removeEventListener("pointerdown", onDown);
      canvas.removeEventListener("pointermove", onMove);
      canvas.removeEventListener("pointerup", onUp);
      canvas.removeEventListener("pointercancel", onUp);
      canvas.removeEventListener("wheel", onWheel);
    },
  };
  return api;
}
