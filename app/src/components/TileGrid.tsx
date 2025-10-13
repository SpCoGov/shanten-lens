import React, { useLayoutEffect, useMemo, useRef, useState } from "react";
import Tile from "./Tile";
import "./tilegrid.css";

/** UI 基准（与裁剪无关） */
const COLS = 9;
const BASE_TILE_W = 64;   // 基准牌宽
const BASE_TILE_H = 84;   // 基准牌高
const BASE_GAP_X = 8;     // 基准列间距
const BASE_GAP_Y = 12;    // 基准行间距

// 最小/最大缩放：不让放大超过 1（基准尺寸），避免撑破卡片
const MIN_SCALE = 0.45;
const MAX_SCALE = 1.0;

export default function TileGrid({ tiles }: { tiles: string[] }) {
    const [hovered, setHovered] = useState<string | null>(null);
    const [scale, setScale] = useState(1);

    // 观察这个容器的“实际可用宽度”，而不是 window 或 grid 自己
    const containerRef = useRef<HTMLDivElement | null>(null);

    useLayoutEffect(() => {
        const el = containerRef.current;
        if (!el) return;

        const calc = (w: number) => {
            // 基于“9 列 + 8 个列间距”需要的基准宽度
            const need =
                COLS * BASE_TILE_W + (COLS - 1) * BASE_GAP_X;

            // 根据容器可用宽度计算 scale，并夹在 [MIN_SCALE, MAX_SCALE]
            const s = Math.min(MAX_SCALE, Math.max(MIN_SCALE, w / need));
            setScale(s);
        };

        // 初次
        calc(el.clientWidth);

        // 观察容器（content-box 尺寸变化）
        const ro = new ResizeObserver((entries) => {
            const w = entries[0]?.contentRect?.width ?? el.clientWidth;
            calc(w);
        });
        ro.observe(el);

        // 窗口变化兜底
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
    const gapX  = Math.round(BASE_GAP_X * scale);
    const gapY  = Math.round(BASE_GAP_Y * scale);

    return (
        <section
            className="card tilegrid-card"
            style={{
                width: "100%",
                boxSizing: "border-box",
                overflow: "hidden", // 双保险：避免偶发像素误差导致溢出
            }}
        >
            <div style={{ fontWeight: 600, marginBottom: 8 }}>牌山（按服务器顺序）</div>

            {/* 只观察这个容器，宽度为卡片内可用空间 */}
            <div ref={containerRef} style={{ width: "100%" }}>
                <div
                    className="tilegrid-grid"
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
                        />
                    ))}
                </div>
            </div>
        </section>
    );
}