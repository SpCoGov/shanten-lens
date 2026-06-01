import React, {useLayoutEffect, useMemo, useRef, useState} from "react";
import "../styles/theme.css";
import Tile from "./Tile";
import "./TileGrid.module.css";
import type {Cell} from "../lib/gamestate";
import Modal from "./Modal";
import {t} from "i18next";
import {ws} from "../lib/ws";

const ROWS = 4;
const COLS = 9;

const BASE_TILE_W = 64;
const BASE_TILE_H = 84;
const BASE_GAP_X = 8;
const BASE_GAP_Y = 12;
const MIN_SCALE = 0.45;
const MAX_SCALE = 1.0;

export default function TileGrid({
    cells,
    tianDoraTiles = [],
    doraCountByTile,
}: {
    cells: Cell[];
    tianDoraTiles?: string[];
    doraCountByTile?: ReadonlyMap<string, number>;
}) {
    const [hovered, setHovered] = useState<string | null>(null);
    const [scale, setScale] = useState(1);
    const [selectedCell, setSelectedCell] = useState<Cell | null>(null);
    const containerRef = useRef<HTMLDivElement | null>(null);

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

    const data = useMemo(() => cells.slice(0, ROWS * COLS), [cells]);

    const tileW = Math.round(BASE_TILE_W * scale);
    const tileH = Math.round(BASE_TILE_H * scale);
    const gapX = Math.round(BASE_GAP_X * scale);
    const gapY = Math.round(BASE_GAP_Y * scale);

    const totalSlots = ROWS * COLS;
    const selectedTileId = selectedCell?.id;

    const discardSelectedTile = () => {
        if (typeof selectedTileId !== "number") return;
        ws.send({type: "discard_tile_by_id", data: {tileId: selectedTileId}});
        setSelectedCell(null);
    };

    return (
        <>
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
                                    color: "var(--muted)",
                                    fontSize: 14,
                                }}
                            >
                                {t("tile_grid.empty")}
                            </div>
                        ) : (
                            data.map((cell, i) => {
                                const posIndex = totalSlots - 1 - i;
                                const row = Math.floor(posIndex / COLS);
                                const col = posIndex % COLS;
                                const hasTileId = typeof cell.id === "number";

                                return (
                                    <div
                                        key={`${cell.tile}-${cell.id ?? i}-${cell.dim ? "d" : "n"}`}
                                        style={{
                                            gridRow: row + 1,
                                            gridColumn: col + 1,
                                            cursor: hasTileId ? "pointer" : undefined,
                                        }}
                                        role={hasTileId ? "button" : undefined}
                                        tabIndex={hasTileId ? 0 : undefined}
                                        title={hasTileId ? t("tile_grid.discard_tile_title", {id: cell.id}) : undefined}
                                        onClick={hasTileId ? () => setSelectedCell(cell) : undefined}
                                        onKeyDown={hasTileId ? (e) => {
                                            if (e.key === "Enter" || e.key === " ") {
                                                e.preventDefault();
                                                setSelectedCell(cell);
                                            }
                                        } : undefined}
                                    >
                                        <Tile
                                            tile={cell.tile}
                                            dim={cell.dim}
                                            hoveredTile={hovered}
                                            setHoveredTile={setHovered}
                                            tianDoraTiles={tianDoraTiles}
                                            doraCountByTile={doraCountByTile}
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

            <Modal
                open={selectedCell !== null}
                onClose={() => setSelectedCell(null)}
                title={t("tile_grid.discard_confirm_title")}
                width={360}
                actions={
                    <button className="btn primary" onClick={discardSelectedTile}>
                        {t("tile_grid.discard_confirm_action")}
                    </button>
                }
            >
                <div style={{display: "grid", gap: 12}}>
                    <div style={{display: "flex", alignItems: "center", gap: 12}}>
                        {selectedCell ? (
                            <Tile
                                tile={selectedCell.tile}
                                dim={selectedCell.dim}
                                hoveredTile={null}
                                width={44}
                                height={58}
                            />
                        ) : null}
                        <div>
                            <div style={{fontWeight: 700}}>
                                {t("tile_grid.tile_id", {id: selectedTileId ?? "-"})}
                            </div>
                            <div style={{color: "var(--muted)", fontSize: 13}}>
                                {t("tile_grid.tile_face", {tile: selectedCell?.tile ?? "-"})}
                            </div>
                        </div>
                    </div>
                </div>
            </Modal>
        </>
    );
}
