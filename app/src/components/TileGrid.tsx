import React, { useLayoutEffect, useMemo, useRef, useState } from "react";
import Tile from "./Tile";

const COLS = 9;
const BASE_TILE_W = 64;
const BASE_TILE_H = 84;
const BASE_GAP_X = 8;
const BASE_GAP_Y = 12;
const MIN_SCALE = 0.5;
const MAX_SCALE = 2.0;

export default function TileGrid({ tiles }: { tiles: string[] }) {
    const [hovered, setHovered] = useState<string | null>(null);
    const [scale, setScale] = useState(1);
    const containerRef = useRef<HTMLDivElement | null>(null);

    useLayoutEffect(() => {
        const el = containerRef.current;
        if (!el) return;

        const calc = (w: number) => {
            const need = COLS * BASE_TILE_W + (COLS - 1) * BASE_GAP_X;
            const s = Math.min(MAX_SCALE, Math.max(MIN_SCALE, w / need));
            setScale(s);
        };

        // 初始
        calc(el.clientWidth);

        // 用 ResizeObserver 观察外层容器（content-box 尺寸）
        const ro = new ResizeObserver((entries) => {
            const e = entries[0];
            // 有些浏览器支持 contentBoxSize，统一用 contentRect
            const w = e.contentRect?.width ?? el.clientWidth;
            calc(w);
        });
        ro.observe(el);

        // 窗口尺寸变化兜底
        const onWin = () => calc(el.clientWidth);
        window.addEventListener("resize", onWin);

        return () => {
            ro.disconnect();
            window.removeEventListener("resize", onWin);
        };
    }, []);

    const data = useMemo(() => tiles.slice(0, 36), [tiles]);

    // 渲染尺寸 = 基准 * scale
    const tileW = Math.round(BASE_TILE_W * scale);
    const tileH = Math.round(BASE_TILE_H * scale);
    const gapX = Math.round(BASE_GAP_X * scale);
    const gapY = Math.round(BASE_GAP_Y * scale);

    return (
        <section className="card" style={{ display: "grid", gap: 10 }}>
            <div style={{ fontWeight: 600 }}>牌山（按服务器顺序）</div>

            {/* 这个 wrap 只负责提供“可用宽度”，供 ResizeObserver 使用 */}
            <div ref={containerRef} style={{ width: "100%" }}>
                <div
                    style={{
                        display: "grid",
                        gridTemplateColumns: `repeat(${COLS}, ${tileW}px)`,
                        gridAutoRows: `${tileH}px`,
                        columnGap: gapX,
                        rowGap: gapY,
                        justifyContent: "center",
                        justifyItems: "center",
                        alignItems: "center",
                        width: "100%",
                    }}
                >
                    {data.map((t, i) => (
                        <Tile
                            key={`${t}-${i}`}
                            tile={t}
                            hoveredTile={hovered}
                            setHoveredTile={setHovered}
                            width={tileW}
                            height={tileH}
                            atlasSrc="/assets/mjp_default.png"
                        />
                    ))}
                </div>
            </div>
        </section>
    );
}