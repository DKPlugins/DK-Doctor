import type { MapEdge, MapGraph, Severity } from "./api";

/**
 * Map-transition graph (D6): a deterministic BFS-layered layout of the map
 * transition graph, rendered to a static SVG. No physics — layers are shortest
 * path from the start map, islands (unreachable from start) drop to a band at the
 * bottom, and edges to a missing map are drawn dashed-red to a red stub.
 *
 * Reachability here is over *direct* transfers only; maps reached solely via
 * by-variable / plugin transfers can appear as islands, so the view labels the
 * island band as "no known entrance" rather than asserting a bug (the
 * `unreachable-maps` rule owns the analytical claim).
 */

const BOX_W = 138;
const BOX_H = 42;
const GAP_X = 26;
const GAP_Y = 78;
const PAD = 28;
/** Above this node count the layout is skipped (too dense to read usefully). */
export const GRAPH_MAX_NODES = 400;

/** Summary counts for the graph header. */
export interface GraphStats {
  nodes: number;
  edges: number;
  reachable: number;
  islands: number;
  /** Edges whose target map does not exist. */
  broken: number;
}

/** Options controlling the SVG render. */
export interface GraphRenderOpts {
  /** Currently-selected map id (highlighted), or null. */
  selected: number | null;
  /** Worst finding severity per map id (for node tinting). */
  worst: Map<number, Severity>;
  /** Tooltip suffix for an island node (localized). */
  islandTitle: string;
  /** Tooltip for a missing (broken-target) stub (localized). */
  brokenTitle: string;
}

/** Adjacency + reachability derived from a graph. */
interface Derived {
  nodeIds: Set<number>;
  /** Shortest-path depth from start over direct edges (undefined = unreachable). */
  depth: Map<number, number>;
  /** Map ids referenced as edge targets but absent from nodes. */
  missing: Set<number>;
}

/** Escapes text for SVG/HTML insertion. */
function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** Builds adjacency, BFS depths from the start map, and the missing-target set. */
function derive(graph: MapGraph): Derived {
  const nodeIds = new Set(graph.nodes.map((n) => n.id));
  const adj = new Map<number, number[]>();
  const missing = new Set<number>();
  for (const e of graph.edges) {
    const arr = adj.get(e.from);
    if (arr) arr.push(e.to);
    else adj.set(e.from, [e.to]);
    if (!nodeIds.has(e.to)) missing.add(e.to);
  }
  const depth = new Map<number, number>();
  // Seed BFS from the start map if it exists, else from every node with no
  // incoming edge (so a project without a valid startMapId still lays out).
  const seeds: number[] = [];
  if (nodeIds.has(graph.startMapId)) {
    seeds.push(graph.startMapId);
  } else {
    const hasIncoming = new Set(graph.edges.map((e) => e.to));
    for (const n of graph.nodes) if (!hasIncoming.has(n.id)) seeds.push(n.id);
  }
  const queue: number[] = [];
  for (const s of seeds) {
    if (!depth.has(s)) {
      depth.set(s, 0);
      queue.push(s);
    }
  }
  for (let head = 0; head < queue.length; head++) {
    const cur = queue[head];
    const d = depth.get(cur)!;
    for (const to of adj.get(cur) ?? []) {
      if (nodeIds.has(to) && !depth.has(to)) {
        depth.set(to, d + 1);
        queue.push(to);
      }
    }
  }
  return { nodeIds, depth, missing };
}

/** Computes header counts for a graph. */
export function graphStats(graph: MapGraph): GraphStats {
  const { depth, missing } = derive(graph);
  const reachable = graph.nodes.filter((n) => depth.has(n.id)).length;
  return {
    nodes: graph.nodes.length,
    edges: graph.edges.length,
    reachable,
    islands: graph.nodes.length - reachable,
    broken: missing.size,
  };
}

/** Position of a laid-out box (top-left corner). */
interface Pos {
  x: number;
  y: number;
}

const SEV_FILL: Record<Severity, string> = {
  error: "var(--sev-error-bg)",
  warning: "var(--sev-warning-bg)",
  info: "var(--sev-info-bg)",
};
const SEV_STROKE: Record<Severity, string> = {
  error: "var(--sev-error)",
  warning: "var(--sev-warning)",
  info: "var(--sev-info)",
};

/**
 * Renders the graph as an SVG string. Nodes carry `data-mapsel` so a click
 * selects that map (handled by the existing atlas delegation). Returns `""` when
 * the graph is empty; a too-dense graph is the caller's responsibility to gate.
 */
export function renderMapGraph(graph: MapGraph, opts: GraphRenderOpts): string {
  if (!graph.nodes.length) return "";
  const { nodeIds, depth, missing } = derive(graph);
  const byName = new Map(graph.nodes.map((n) => [n.id, n]));

  // Bucket reachable nodes by depth; islands share one band after the deepest
  // reachable layer; missing stubs go on a final band.
  const layers = new Map<number, number[]>();
  const islands: number[] = [];
  let maxDepth = 0;
  for (const n of graph.nodes) {
    const d = depth.get(n.id);
    if (d === undefined) {
      islands.push(n.id);
    } else {
      maxDepth = Math.max(maxDepth, d);
      const arr = layers.get(d);
      if (arr) arr.push(n.id);
      else layers.set(d, [n.id]);
    }
  }
  const bands: { ids: number[]; kind: "layer" | "island" | "missing" }[] = [];
  for (let d = 0; d <= maxDepth; d++) {
    const ids = (layers.get(d) ?? []).slice().sort((a, b) => a - b);
    if (ids.length) bands.push({ ids, kind: "layer" });
  }
  if (islands.length) bands.push({ ids: islands.slice().sort((a, b) => a - b), kind: "island" });
  const missingIds = [...missing].sort((a, b) => a - b);
  if (missingIds.length) bands.push({ ids: missingIds, kind: "missing" });

  // Assign positions row by row, centering each row within the widest one.
  const rowWidth = (n: number) => n * BOX_W + (n - 1) * GAP_X;
  const widest = Math.max(1, ...bands.map((b) => rowWidth(b.ids.length)));
  const pos = new Map<number, Pos>();
  bands.forEach((band, row) => {
    const w = rowWidth(band.ids.length);
    const x0 = PAD + (widest - w) / 2;
    band.ids.forEach((id, i) => {
      pos.set(id, { x: x0 + i * (BOX_W + GAP_X), y: PAD + row * GAP_Y });
    });
  });
  const svgW = widest + PAD * 2;
  const svgH = PAD * 2 + bands.length * GAP_Y - (GAP_Y - BOX_H);

  const cx = (id: number) => (pos.get(id)?.x ?? 0) + BOX_W / 2;
  const topY = (id: number) => pos.get(id)?.y ?? 0;
  const botY = (id: number) => (pos.get(id)?.y ?? 0) + BOX_H;

  // Edges first (under the nodes).
  let edgeSvg = "";
  const drawn = new Set<string>();
  for (const e of graph.edges) {
    const key = `${e.from}->${e.to}`;
    if (drawn.has(key)) continue;
    drawn.add(key);
    if (!pos.has(e.from) || !pos.has(e.to)) continue;
    const broken = !nodeIds.has(e.to);
    const x1 = cx(e.from);
    const x2 = cx(e.to);
    // Route downward normally; a back/self edge curves to the side.
    const forward = topY(e.to) > botY(e.from);
    const y1 = forward ? botY(e.from) : topY(e.from);
    const y2 = forward ? topY(e.to) : botY(e.to);
    const my = (y1 + y2) / 2;
    const cls = broken ? "gedge gedge--broken" : "gedge";
    edgeSvg += `<path class="${cls}" d="M${x1},${y1} C${x1},${my} ${x2},${my} ${x2},${y2}"/>`;
  }

  // Nodes.
  let nodeSvg = "";
  for (const band of bands) {
    for (const id of band.ids) {
      const p = pos.get(id)!;
      const isMissing = band.kind === "missing";
      const node = byName.get(id);
      const isStart = id === graph.startMapId && !isMissing;
      const sev = opts.worst.get(id) ?? null;
      let fill = "var(--surface)";
      let stroke = "var(--border-strong)";
      if (isMissing) {
        fill = "var(--sev-error-bg)";
        stroke = "var(--sev-error)";
      } else if (sev) {
        fill = SEV_FILL[sev];
        stroke = SEV_STROKE[sev];
      }
      const label = isMissing
        ? `Map ${id}?`
        : node?.name
          ? node.name
          : `Map ${id}`;
      const title = isMissing
        ? opts.brokenTitle
        : band.kind === "island"
          ? opts.islandTitle
          : label;
      const cls =
        "gnode" +
        (opts.selected === id ? " is-active" : "") +
        (isStart ? " gnode--start" : "") +
        (isMissing ? " gnode--missing" : "");
      const dyn = node && node.dynamicExits > 0 ? `<title>+${node.dynamicExits} dynamic</title>` : "";
      const sel = isMissing ? "" : ` data-mapsel="${id}"`;
      nodeSvg +=
        `<g class="${cls}"${sel} tabindex="0">` +
        `<title>${esc(title)}</title>${dyn}` +
        `<rect x="${p.x}" y="${p.y}" width="${BOX_W}" height="${BOX_H}" rx="8" ` +
        `fill="${fill}" stroke="${stroke}"/>` +
        (isStart ? `<circle cx="${p.x + 12}" cy="${p.y + BOX_H / 2}" r="4" fill="var(--brand)"/>` : "") +
        `<text x="${p.x + (isStart ? 24 : BOX_W / 2)}" y="${p.y + BOX_H / 2 + 4}" ` +
        `text-anchor="${isStart ? "start" : "middle"}" class="glabel">` +
        `<tspan class="gid">${id}</tspan> ${esc(label.length > 14 ? label.slice(0, 13) + "…" : label)}</text>` +
        "</g>";
    }
  }

  return (
    `<svg class="mapgraph__svg" viewBox="0 0 ${Math.ceil(svgW)} ${Math.ceil(svgH)}" ` +
    `width="${Math.ceil(svgW)}" height="${Math.ceil(svgH)}" xmlns="http://www.w3.org/2000/svg">` +
    edgeSvg +
    nodeSvg +
    "</svg>"
  );
}

/** Whether a graph is small enough to lay out (see {@link GRAPH_MAX_NODES}). */
export function graphIsRenderable(graph: MapGraph): boolean {
  return graph.nodes.length > 0 && graph.nodes.length <= GRAPH_MAX_NODES;
}

/** Convenience: total distinct edges for the header (already deduped by adapter). */
export function edgeCount(edges: MapEdge[]): number {
  return edges.length;
}
