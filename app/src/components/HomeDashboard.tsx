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

type Breakpoint = "lg";
type StoredLayout = {schema: 4; layouts: ResponsiveLayouts};

const STORAGE_KEY = "shanten-lens.home-layout";
// 主界面只使用一套网格布局，调整窗口大小时不再切换并改写另一套组件尺寸。
const BREAKPOINTS = {lg: 0};
const COLS = {lg: 12};
const DEFAULT_WIDTH = 4;
// react-grid-layout 的 3 个纵向小行约等于界面中 1 个正方形单位。
const ROWS_PER_UNIT = 3;

type TileSize = {
    default: {w: number; h: number};
    min: {w: number; h: number};
    max: {w: number | null; h: number | null};
};

// 所有组件的尺寸只在这里定义：default=默认，min=最小，max=最大；max 中的 null 表示不限制。
// w 使用网格列数，h 使用纵向小行数。需要按视觉方格设置高度时，用 ROWS_PER_UNIT 换算。
const TILE_SIZES: Record<string, TileSize> = {
    // 左侧信息
    "side-content":            {default: {w: DEFAULT_WIDTH, h: 6}, min: {w: 2, h: 3}, max: {w: DEFAULT_WIDTH, h: null}}, // 关卡建议/历史分数
    "side-health":             {default: {w: DEFAULT_WIDTH, h: 2}, min: {w: 2, h: 2}, max: {w: DEFAULT_WIDTH, h: 2}}, // 角色血量

    // 主区域：护身符、商店、手牌和替换序列
    "main:amulets":            {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 4, h: 5}, max: {w: null, h: null}}, // 护身符
    "main:goods":              {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 1, h: ROWS_PER_UNIT}, max: {w: null, h: null}}, // 商品
    "main:shop-buff":          {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 1, h: ROWS_PER_UNIT}, max: {w: null, h: null}}, // 商店增益
    "main:candidates":         {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 1, h: ROWS_PER_UNIT}, max: {w: null, h: null}}, // 推荐候选
    "main:tiles":              {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 3, h: 2 * ROWS_PER_UNIT}, max: {w: null, h: null}}, // 手牌
    "main:replacement":        {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 1, h: ROWS_PER_UNIT}, max: {w: null, h: null}}, // 替换序列

    // 统计区域：宝牌、替换统计和牌山统计
    "stats:dora":              {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 2, h: ROWS_PER_UNIT}, max: {w: null, h: null}}, // 宝牌指示牌
    "stats:replacement-stats": {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 1, h: ROWS_PER_UNIT}, max: {w: null, h: null}}, // 替换统计
    "stats:wall-stats":        {default: {w: DEFAULT_WIDTH, h: 9}, min: {w: 3, h: 2 * ROWS_PER_UNIT}, max: {w: null, h: null}}, // 牌山统计
};

function tileSize(id: string, cols: number) {
    const size = TILE_SIZES[id] ?? {
        default: {w: DEFAULT_WIDTH, h: 9},
        min: {w: 1, h: ROWS_PER_UNIT},
        max: {w: null, h: null},
    };
    return {
        w: Math.min(size.default.w, cols),
        h: size.default.h,
        minW: Math.min(size.min.w, cols),
        minH: size.min.h,
        ...(size.max.w == null ? {} : {maxW: Math.min(size.max.w, cols)}),
        ...(size.max.h == null ? {} : {maxH: size.max.h}),
    };
}

function normalizeTileId(id: string) {
    const separator = id.indexOf(":");
    const reactKeyMarker = id.lastIndexOf("$");
    if (separator >= 0 && reactKeyMarker > separator) {
        return `${id.slice(0, separator + 1)}${id.slice(reactKeyMarker + 1)}`;
    }
    return id;
}

function defaults(ids: string[]): ResponsiveLayouts {
    return Object.fromEntries((Object.keys(COLS) as Breakpoint[]).map((bp) => {
        const cols = COLS[bp];
        const hasSideContent = ids.includes("side-content");
        const hasSideHealth = ids.includes("side-health");
        const hasSide = hasSideContent || hasSideHealth;
        const regularIds = ids.filter((id) => id !== "side-content" && id !== "side-health");
        return [bp, ids.map((id) => {
            const size = tileSize(id, cols);
            if (id === "side-content") {
                return {i: id, x: 0, y: 0, ...size};
            }
            if (id === "side-health") {
                return {i: id, x: 0, y: hasSideContent ? TILE_SIZES["side-content"].default.h : 0, ...size};
            }
            const regularIndex = regularIds.indexOf(id);
            const index = regularIndex + (hasSide ? 1 : 0);
            const y = Math.floor(index * size.w / cols) * size.h;
            return {
                i: id, x: (index * size.w) % cols, y, ...size,
            };
        })];
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
            if (!initial) return item;
            const minW = initial.minW ?? 1;
            const maxW = initial.maxW ?? Infinity;
            const minH = initial.minH ?? 1;
            const maxH = initial.maxH ?? Infinity;
            return {
                ...initial,
                ...item,
                w: Math.max(minW, Math.min(item.w, maxW)),
                h: Math.max(minH, Math.min(item.h, maxH)),
                minW: initial.minW,
                maxW: initial.maxW,
                minH: initial.minH,
                maxH: initial.maxH,
            };
        });
        return [bp, [...reconciled, ...(base[bp] ?? []).filter((item) => !currentIds.has(item.i))]];
    }));
}

function loadState(ids: string[]): StoredLayout {
    try {
        const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null") as {schema?: number; layouts?: ResponsiveLayouts} | null;
        if ((parsed?.schema === 2 || parsed?.schema === 3 || parsed?.schema === 4) && parsed.layouts) {
            const layouts = Object.fromEntries(Object.entries(parsed.layouts).map(([bp, layout]) => [
                bp,
                layout?.map((item) => ({...item, i: normalizeTileId(item.i)})),
            ])) as ResponsiveLayouts;
            return {schema: 4, layouts: reconcileLayouts(layouts, ids)};
        }
    } catch {
        // Invalid or unavailable local storage falls back to the shipped layout.
    }
    return {schema: 4, layouts: defaults(ids)};
}

function expandTiles(tiles: Array<HomeTile | null>): HomeTile[] {
    return tiles.filter((tile): tile is HomeTile => tile !== null).flatMap((tile) => {
        if (!tile.splitChildren || !React.isValidElement<{children?: React.ReactNode}>(tile.content)) return [tile];
        return React.Children.toArray(tile.content.props.children).map((content, index) => ({
            id: normalizeTileId(`${tile.id}:${React.isValidElement(content) && content.key != null ? content.key : index}`),
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
            localStorage.setItem(STORAGE_KEY, JSON.stringify({schema: 4, layouts: nextLayouts}));
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
