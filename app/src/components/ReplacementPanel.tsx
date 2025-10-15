import React from "react";
import Tile from "./Tile";

const emitHover = (tile: string | null) =>
    window.dispatchEvent(new CustomEvent("shanten:hover-tile", { detail: tile }));

export default function ReplacementPanel({
                                             replacementTiles,
                                             usedCount,
                                         }: {
    replacementTiles: string[];
    usedCount: number;
}) {
    const [hovered, setHovered] = React.useState<string | null>(null);
    const scrollRef = React.useRef<HTMLDivElement | null>(null);

    React.useEffect(() => {
        const onHover = (e: Event) => {
            const ce = e as CustomEvent<string | null>;
            setHovered(ce.detail ?? null);
        };
        window.addEventListener("shanten:hover-tile", onHover as EventListener);
        return () => window.removeEventListener("shanten:hover-tile", onHover as EventListener);
    }, []);

    const rafRef = React.useRef<number | null>(null);
    const targetRef = React.useRef(0);

    const animate = React.useCallback(() => {
        const el = scrollRef.current;
        if (!el) return;
        const current = el.scrollLeft;
        const target = targetRef.current;
        const diff = target - current;

        if (Math.abs(diff) < 0.5) {
            el.scrollLeft = target;
            if (rafRef.current) cancelAnimationFrame(rafRef.current);
            rafRef.current = null;
            return;
        }
        el.scrollLeft = current + diff * 0.22; // 缓动系数：越大越快
        rafRef.current = requestAnimationFrame(animate);
    }, []);

    const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
        const el = scrollRef.current;
        if (!el) return;
        const multiplier = 1.3;
        const max = el.scrollWidth - el.clientWidth;

        targetRef.current = Math.max(0, Math.min(max, targetRef.current + e.deltaY * multiplier));
        if (rafRef.current == null) rafRef.current = requestAnimationFrame(animate);
        e.preventDefault();
    };

    React.useEffect(() => {
        const el = scrollRef.current;
        if (el) targetRef.current = el.scrollLeft;
        return () => {
            if (rafRef.current) cancelAnimationFrame(rafRef.current);
        };
    }, []);

    const lastIdx = Math.max(-1, usedCount - 1);

    return (
        <section className="mj-panel" style={{ marginTop: 12, overflowY: "hidden" }}>
            <div style={{ marginBottom: 8, display: "flex", alignItems: "baseline", gap: 8 }}>
                <div style={{ fontWeight: 600 }}>替换序列</div>
                <div style={{ fontSize: 12, color: "#6b7280" }}>
                    可替换 {replacementTiles.length} 张 · 已替换 {usedCount} 张
                </div>
            </div>

            <div
                ref={scrollRef}
                onWheel={onWheel}
                style={{
                    display: "grid",
                    gridAutoFlow: "column",
                    gridAutoColumns: "min-content",
                    gap: 10,
                    overflowX: "auto",
                    overflowY: "hidden",
                    paddingRight: 5,
                    paddingLeft: 5,
                    paddingBottom: 8,
                    paddingTop: 8,
                    overscrollBehaviorX: "contain",
                }}
            >
                {replacementTiles.map((t, idx) => {
                    const used = idx <= lastIdx;
                    return (
                        <div key={`${t}-${idx}`} style={{ display: "grid", justifyItems: "center" }}>
                            <div
                                style={{
                                    position: "relative",
                                    borderRadius: 8,
                                    outline: used ? "2px solid rgba(16,185,129,.85)" : "none",
                                    outlineOffset: used ? 2 : 0,
                                    marginBottom: 10,
                                }}
                                onMouseEnter={() => emitHover(t)}
                                onMouseLeave={() => emitHover(null)}
                                onClick={() => emitHover(t)}
                                title={`${t}${used ? "（已替）" : ""}`}
                            >
                                <Tile
                                    tile={t}
                                    dim={false}
                                    hoveredTile={hovered}
                                    setHoveredTile={(x) => emitHover(x)}
                                    width={54}
                                    height={72}
                                />

                                {used && (
                                    <div
                                        style={{
                                            position: "absolute",
                                            top: "50%",
                                            left: "50%",
                                            transform: "translate(-50%, -50%)",
                                            fontSize: 13,
                                            fontWeight: 700,
                                            color: "#fff",
                                            background: "rgba(16,185,129,0.88)",
                                            padding: "4px 10px",
                                            borderRadius: 8,
                                            boxShadow: "0 0 6px rgba(0,0,0,0.25)",
                                            pointerEvents: "none",
                                        }}
                                    >
                                        已替
                                    </div>
                                )}
                            </div>

                            <div style={{ height: 18, display: "grid", placeItems: "center" }}>
                                {idx === lastIdx && usedCount > 0 && (
                                    <div style={{ display: "grid", justifyItems: "center", gap: 2 }}>
                                        <div
                                            style={{
                                                width: 0,
                                                height: 0,
                                                borderLeft: "6px solid transparent",
                                                borderRight: "6px solid transparent",
                                                borderTop: "8px solid rgba(59,130,246,.9)",
                                            }}
                                        />
                                        <div style={{ fontSize: 11, color: "rgba(59,130,246,.9)" }}>当前</div>
                                    </div>
                                )}
                            </div>
                        </div>
                    );
                })}
            </div>
        </section>
    );
}