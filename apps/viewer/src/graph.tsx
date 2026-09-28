import { useEffect, useMemo, useRef, useState } from "react";
import type { ExecutionEvent } from "../../../packages/typescript/src/index.js";
export function layout(events: ExecutionEvent[]) {
  const depth = new Map<string, number>();
  return events.map((event, index) => {
    const level = event.parent_id ? (depth.get(event.parent_id) ?? -1) + 1 : 0;
    depth.set(event.id, level);
    return { event, x: 24 + Math.min(level, 32) * 250, y: 24 + index * 78 };
  });
}
/** Render only rows intersecting the viewport, with a small keyboard/scroll overscan. */
export function visibleRows(
  count: number,
  top: number,
  height: number,
  zoom: number,
) {
  const first = Math.max(0, Math.floor(top / zoom / 78) - 4);
  const last = Math.min(count, Math.ceil((top + height) / zoom / 78) + 4);
  return [first, last] as const;
}
export function ExecutionGraph({
  events,
  selected,
  onSelect,
}: {
  events: ExecutionEvent[];
  selected?: string;
  onSelect: (event: ExecutionEvent) => void;
}) {
  const [zoom, setZoom] = useState(1);
  const viewport = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ top: 0, height: 500 });
  const nodes = useMemo(() => layout(events), [events]);
  const byId = useMemo(
    () => new Map(nodes.map((node) => [node.event.id, node])),
    [nodes],
  );
  const width = useMemo(
    () => nodes.reduce((max, node) => Math.max(max, node.x + 242), 560),
    [nodes],
  );
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(() =>
      setPosition({ top: element.scrollTop, height: element.clientHeight }),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const node = selected ? byId.get(selected) : undefined;
    const element = viewport.current;
    if (!element || !node) return;
    const y = node.y * zoom;
    if (
      y < element.scrollTop ||
      y + 54 * zoom > element.scrollTop + element.clientHeight
    ) {
      element.scrollTop = Math.max(0, y - 78 * zoom);
      setPosition({ top: element.scrollTop, height: element.clientHeight });
    }
  }, [selected, byId, zoom]);
  const [first, last] = visibleRows(
    nodes.length,
    position.top,
    position.height,
    zoom,
  );
  const visible = nodes.slice(first, last);
  const height = Math.max(130, nodes.length * 78 + 32);
  return (
    <section className="graph-panel" aria-label="Execution graph">
      <div className="panel-heading">
        <h2>Execution graph</h2>
        <label>
          Zoom{" "}
          <input
            aria-label="Graph zoom"
            type="range"
            min="0.5"
            max="1.5"
            step="0.1"
            value={zoom}
            onChange={(event) => setZoom(Number(event.target.value))}
          />
        </label>
      </div>
      <div
        className="graph-scroll"
        ref={viewport}
        onScroll={(event) =>
          setPosition({
            top: event.currentTarget.scrollTop,
            height: event.currentTarget.clientHeight,
          })
        }
      >
        <svg
          viewBox={`0 0 ${width} ${height}`}
          width={width * zoom}
          height={height * zoom}
          role="group"
          aria-label="Causal parent and child event graph"
        >
          <defs>
            <marker
              id="arrow"
              markerWidth="8"
              markerHeight="8"
              refX="7"
              refY="4"
              orient="auto"
            >
              <path d="M0,0 L8,4 L0,8" fill="#77bca5" />
            </marker>
          </defs>
          {visible.map((node) => {
            const parent = node.event.parent_id
              ? byId.get(node.event.parent_id)
              : undefined;
            return parent ? (
              <path
                key={`edge-${node.event.id}`}
                d={`M${parent.x + 212},${parent.y + 26} C${parent.x + 238},${parent.y + 26} ${node.x - 18},${node.y + 26} ${node.x - 5},${node.y + 26}`}
                className="graph-edge"
                markerEnd="url(#arrow)"
              />
            ) : null;
          })}
          {visible.map(({ event, x, y }) => (
            <g
              key={event.id}
              transform={`translate(${x},${y})`}
              role="button"
              tabIndex={0}
              aria-label={`Graph event ${event.name}`}
              aria-pressed={selected === event.id}
              onClick={() => onSelect(event)}
              onKeyDown={(key) => {
                if (key.key === "Enter" || key.key === " ") {
                  key.preventDefault();
                  onSelect(event);
                }
              }}
              className={`graph-node ${selected === event.id ? "selected" : ""} ${event.status}`}
            >
              <title>
                {event.name} · {event.type} · {event.duration_ms ?? 0} ms
              </title>
              <rect width="212" height="54" rx="7" />
              <circle cx="13" cy="17" r="3" />
              <text x="24" y="21">
                {event.name.length > 23
                  ? `${event.name.slice(0, 22)}…`
                  : event.name}
              </text>
              <text x="12" y="42" className="graph-detail">
                {event.type} · {(event.duration_ms ?? 0).toFixed(1)} ms
              </text>
            </g>
          ))}
        </svg>
        {!events.length && <p className="muted">No recorded events.</p>}
      </div>
      <p className="graph-caption">
        Edges show recorded parent relationships. Independent roots have no
        inferred edges. Select a node to inspect its input, output, and metrics
        below. Deep nesting beyond 32 levels shares a column; scroll to reveal
        more events.
      </p>
    </section>
  );
}
