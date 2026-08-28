import React from "react";
import {
    Responsive,
    verticalCompactor,
    type Layout,
    type ResponsiveLayouts,
} from "react-grid-layout";
import "react-grid-layout/css/styles.css";
import "./HomeDashboard.css";

export type HomeTile = {
    id: string;
    content: React.ReactNode;
    splitChildren?: boolean;
};

type Breakpoint = "lg" | "md" | "sm" | "xs";
type StoredLayout = {schema: 2; layouts: ResponsiveLayouts};

const STORAGE_KEY = "shanten-lens.home-layout";
const BREAKPOINTS = {lg: 1200, md: 850, sm: 560, xs: 0};
const COLS = {lg: 12, md: 10, sm: 6, xs: 2};

function defaults(ids: string[]): ResponsiveLayouts {
    return Object.fromEntries((Object.keys(COLS) as Breakpoint[]).map((bp) => {
        const cols = COLS[bp];
        const width = bp === "xs" ? cols : bp === "sm" ? 3 : bp === "md" ? 5 : 4;
        return [bp, ids.map((id, index) => ({
            i: id, x: (index * width) % cols, y: Math.floor(index * width / cols) * 9,
            w: Math.min(width, cols), h: id === "side" ? 14 : 9, minW: 1, minH: 3,
        }))];
    }));
}

function reconcileLayouts(layouts: ResponsiveLayouts, ids: string[]): ResponsiveLayouts {
    const base = defaults(ids);
    return Object.fromEntries((Object.keys(COLS) as Breakpoint[]).map((bp) => {
        const current = layouts[bp] ?? [];
        const baseById = new Map((base[bp] ?? []).map((item) => [item.i, item]));
        const currentIds = new Set(current.map((item) => item.i));
        const reconciled = current.map((item) => {
            const initial = baseById.get(item.i);
            return initial ? {...initial, ...item, minW: initial.minW, minH: initial.minH} : item;
        });
        return [bp, [...reconciled, ...(base[bp] ?? []).filter((item) => !currentIds.has(item.i))]];
    }));
}

function loadState(ids: string[]): StoredLayout {
    try {
        const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null") as Partial<StoredLayout> | null;
        if (parsed?.schema === 2 && parsed.layouts) {
            return {schema: 2, layouts: reconcileLayouts(parsed.layouts, ids)};
        }
    } catch {
        // Invalid or unavailable local storage falls back to the shipped layout.
    }
    return {schema: 2, layouts: defaults(ids)};
}

function expandTiles(tiles: Array<HomeTile | null>): HomeTile[] {
    return tiles.filter((tile): tile is HomeTile => tile !== null).flatMap((tile) => {
        if (!tile.splitChildren || !React.isValidElement<{children?: React.ReactNode}>(tile.content)) return [tile];
        return React.Children.toArray(tile.content.props.children).map((content, index) => ({
            id: `${tile.id}:${React.isValidElement(content) && content.key != null ? content.key : index}`,
            content,
        }));
    });
}

export default function HomeDashboard({tiles, width, mounted}: {
    tiles: Array<HomeTile | null>;
    width: number;
    mounted: boolean;
}) {
    const available = React.useMemo(() => expandTiles(tiles), [tiles]);
    const ids = available.map((tile) => tile.id);
    const idsKey = ids.join("|");
    const [layouts, setLayouts] = React.useState<ResponsiveLayouts>(() => loadState(ids).layouts);
    const visibleIds = React.useMemo(() => new Set(ids), [idsKey]);
    const currentLayouts = React.useMemo(() => reconcileLayouts(layouts, ids), [layouts, idsKey]);
    const visibleLayouts = React.useMemo(() => Object.fromEntries(
        (Object.keys(COLS) as Breakpoint[]).map((bp) => [
            bp,
            (currentLayouts[bp] ?? []).filter((item) => visibleIds.has(item.i)),
        ]),
    ), [currentLayouts, visibleIds]);
    const layoutsRef = React.useRef(currentLayouts);
    layoutsRef.current = currentLayouts;

    const persist = React.useCallback((nextLayouts: ResponsiveLayouts) => {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify({schema: 2, layouts: nextLayouts}));
        } catch {
            // The dashboard remains usable when persistence is unavailable.
        }
    }, []);

    const updateLayouts = React.useCallback((_current: Layout, next: ResponsiveLayouts) => {
        const merged = Object.fromEntries((Object.keys(COLS) as Breakpoint[]).map((bp) => {
            const incoming = next[bp];
            if (!incoming) return [bp, layoutsRef.current[bp] ?? []];
            const incomingIds = new Set(incoming.map((item) => item.i));
            const kept = (layoutsRef.current[bp] ?? []).filter((item) => !incomingIds.has(item.i));
            return [bp, [...incoming, ...kept]];
        }));
        if (JSON.stringify(merged) === JSON.stringify(layoutsRef.current)) return;
        layoutsRef.current = merged;
        setLayouts(merged);
        persist(merged);
    }, [persist]);

    if (!mounted) return null;
    return (
        <Responsive
            className="home-dashboard"
            width={width}
            breakpoints={BREAKPOINTS}
            cols={COLS}
            rowHeight={30}
            margin={[12, 12]}
            containerPadding={[16, 16]}
            layouts={visibleLayouts}
            dragConfig={{cancel: "button, a, input, textarea, select, option, [contenteditable='true'], .amulet-drag-item"}}
            resizeConfig={{handles: ["se"]}}
            compactor={verticalCompactor}
            onLayoutChange={updateLayouts}
        >
            {available.map((tile) => (
                <div className="home-dashboard-tile" key={tile.id}>
                    <div className="home-dashboard-tile-content">{tile.content}</div>
                </div>
            ))}
        </Responsive>
    );
}
