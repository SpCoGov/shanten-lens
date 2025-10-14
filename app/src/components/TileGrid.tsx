import React, {useLayoutEffect, useMemo, useRef, useState} from "react";
import Tile from "./Tile";
import "./tilegrid.css";
import type {Cell} from "../lib/gamestate";

const ROWS = 4;
const COLS = 9;

const BASE_TILE_W = 64;
const BASE_TILE_H = 84;
const BASE_GAP_X = 8;
const BASE_GAP_Y = 12;
const MIN_SCALE = 0.45;
const MAX_SCALE = 1.0;

export default function TileGrid({cells}: { cells: Cell[] }) {
    const [hovered, setHovered] = useState<string | null>(null);
    const [scale, setScale] = useState(1);
    const containerRef = useRef<HTMLDivElement | null>(null);

    // 侧栏/其它处发出的单值 hover 事件（例如 "5p" 或 "0p"）
    React.useEffect(() => {
        const onHover = (e: Event) => {
            const ce = e as CustomEvent<string | null>;
            setHovered(ce.detail ?? null);
        };
        window.addEventListener("shanten:hover-tile", onHover as EventListener);
        return () => window.removeEventListener("shanten:hover-tile", onHover as EventListener);
    }, []);
    React.useEffect(() => {
        window.dispatchEvent(new CustomEvent("shanten:hover-tile", {detail: hovered}));
    }, [hovered]);

    useLayoutEffect(() => {
        const el = containerRef.current;
        if (!el) return;

        const calc = (w: number) => {
            const need = COLS * BASE_TILE_W + (COLS - 1) * BASE_GAP_X;
            setScale(Math.min(MAX_SCALE, Math.max(MIN_SCALE, w / need)));
        };

        calc(el.clientWidth);
        const ro = new ResizeObserver((entries) => {
            const w = entries[0]?.contentRect?.width ?? el.clientWidth;
            calc(w);
        });
        ro.observe(el);
        const onWin = () => calc(el.clientWidth);
        window.addEventListener("resize", onWin);
        return () => {
            ro.disconnect();
            window.removeEventListener("resize", onWin);
        };
    }, []);

    // 截断到 36 张
    const data = useMemo(() => cells.slice(0, ROWS * COLS), [cells]);

    const tileW = Math.round(BASE_TILE_W * scale);
    const tileH = Math.round(BASE_TILE_H * scale);
    const gapX = Math.round(BASE_GAP_X * scale);
    const gapY = Math.round(BASE_GAP_Y * scale);

    const totalSlots = ROWS * COLS;

    return (
        <section
            className="mj-panel card tilegrid-card"
            style={{
                width: "100%",
                boxSizing: "border-box",
                overflow: "hidden",
            }}
        >
            <div ref={containerRef} style={{width: "100%"}}>
                <div
                    className="tilegrid-grid"
                    style={{
                        position: "relative",
                        display: "grid",
                        gridTemplateColumns: `repeat(${COLS}, ${tileW}px)`,
                        gridTemplateRows: `repeat(${ROWS}, ${tileH}px)`,
                        columnGap: gapX,
                        rowGap: gapY,
                        justifyContent: "center",
                        width: "100%",
                        minHeight: ROWS * tileH + (ROWS - 1) * gapY,
                    }}
                >
                    {data.length === 0 ? (
                        <div
                            style={{
                                position: "absolute",
                                inset: 0,
                                display: "grid",
                                placeItems: "center",
                                color: "#6b7280",
                                fontSize: 14,
                            }}
                        >
                            牌山为空
                        </div>
                    ) : (
                        data.map((c, i) => {
                            // 让第 0 个元素落在右下角；随后依次向左、向上回填
                            const posIndex = totalSlots - 1 - i;
                            const r = Math.floor(posIndex / COLS);
                            const cIdx = posIndex % COLS;

                            return (
                                <div
                                    key={`${c.tile}-${c.dim ? "d" : "n"}-${i}`}
                                    style={{gridRow: r + 1, gridColumn: cIdx + 1}}
                                >
                                    <Tile
                                        tile={c.tile}
                                        dim={c.dim}
                                        hoveredTile={hovered}
                                        setHoveredTile={setHovered}
                                        width={tileW}
                                        height={tileH}
                                    />
                                </div>
                            );
                        })
                    )}
                </div>
            </div>
        </section>
    );
}