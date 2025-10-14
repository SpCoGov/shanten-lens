import React from "react";
import { TILE_ATLAS_DEFAULT, TileCode } from "./useCroppedAtlasDefault";

const ATLAS_SRC = "/assets/mjp_default.png";
const LAIZI_SRC = "/assets/mjp_laizi.png";

/** 规范化到项目内合法编码；不含 'bd'，因此癞子要单独判定 */
function normalize(code: string): TileCode {
    const s = code.trim();
    const ok = [
        "0m","1m","2m","3m","4m","5m","6m","7m","8m","9m",
        "0p","1p","2p","3p","4p","5p","6p","7p","8p","9p",
        "0s","1s","2s","3s","4s","5s","6s","7s","8s","9s",
        "1z","2z","3z","4z","5z","6z","7z",
    ] as const;
    return (ok as readonly string[]).includes(s) ? (s as TileCode) : "5m";
}

/** 把原始字符串标准化到项目编码体系（含把中文字牌映射为 z1..z7） */
function normalizeLoose(raw: string): string {
    const s = raw?.trim() ?? "";
    const honor: Record<string, string> = { 东: "z1", 南: "z2", 西: "z3", 北: "z4", 白: "z5", 发: "z6", 中: "z7" };
    if (honor[s]) return honor[s];
    if (/^[mps][0-9]$/.test(s)) return s;               // m0..m9 / p0..p9 / s0..s9
    if (/^[0-9][mps]$/.test(s)) return `${s[1]}${s[0]}`; // 1m..9m / 0p..9p
    if (/^z[1-7]$/.test(s)) return s;
    return s;
}

function parseSuitVal(n: string): { suit: string|null; val: number|null } {
    if (/^[mps][0-9]$/.test(n)) return { suit: n[0], val: Number(n[1]) };
    if (/^z[1-7]$/.test(n)) return { suit: "z", val: Number(n[1]) };
    return { suit: null, val: null };
}

/** 等价判定：同花色 && (同值 || 0↔5)；字牌需完全一致 */
function isEquivalent(aRaw: string, bRaw: string): boolean {
    const a = normalizeLoose(aRaw);
    const b = normalizeLoose(bRaw);
    if (a === b) return true;

    const pa = parseSuitVal(a);
    const pb = parseSuitVal(b);
    if (!pa.suit || !pb.suit) return false;
    if (pa.suit !== pb.suit) return false;

    if (pa.suit === "z") return a === b; // 字牌必须完全一致
    if (pa.val === pb.val) return true;
    return new Set([pa.val, pb.val]).has(0) && new Set([pa.val, pb.val]).has(5);
}

const atlasCache: { img?: HTMLImageElement; ready: boolean; cbs: Array<() => void> } = {
    img: undefined,
    ready: false,
    cbs: [],
};

function loadAtlas(onReady: () => void) {
    if (atlasCache.ready) return onReady();
    atlasCache.cbs.push(onReady);
    if (!atlasCache.img) {
        const img = new Image();
        img.src = ATLAS_SRC;
        img.onload = () => {
            atlasCache.ready = true;
            atlasCache.cbs.splice(0).forEach((fn) => fn());
        };
        atlasCache.img = img;
    }
}

export default function Tile({
                                 tile,
                                 hoveredTile,
                                 setHoveredTile,
                                 width = 64,
                                 height = 84,
                                 dim = false,
                             }: {
    tile: string;
    hoveredTile?: string | null;
    setHoveredTile?: (t: string | null) => void;
    width?: number;
    height?: number;
    dim?: boolean;
}) {
    const raw = tile?.trim() ?? "";
    const isLaizi = raw === "bd";                  // 先对原始值判断癞子
    const norm = isLaizi ? "5m" : normalize(raw);  // 癞子不参与 atlas 映射时的等价判断
    const canvasRef = React.useRef<HTMLCanvasElement | null>(null);

    // === 等价组高亮：hoveredTile 命中当前牌或其等价（赤五↔五）即高亮 ===
    const active = hoveredTile ? isEquivalent(hoveredTile, raw) : false;

    React.useEffect(() => {
        if (isLaizi) return;
        const crop = TILE_ATLAS_DEFAULT[norm] || TILE_ATLAS_DEFAULT["5m"];
        const draw = () => {
            const cvs = canvasRef.current;
            const img = atlasCache.img!;
            if (!cvs || !img) return;
            cvs.width = width;
            cvs.height = height;
            const ctx = cvs.getContext("2d")!;
            ctx.clearRect(0, 0, width, height);
            ctx.imageSmoothingEnabled = true;
            ctx.imageSmoothingQuality = "high";
            ctx.drawImage(img, crop.x, crop.y, crop.w, crop.h, 0, 0, width, height);
        };
        loadAtlas(draw);
        if (atlasCache.ready) draw();
    }, [norm, width, height, isLaizi]);

    return (
        <div
            className={`mj-tile ${active ? "mj-tile--highlight" : ""}`}
            style={{
                position: "relative",
                width,
                height,
                borderRadius: 8,
                background: "#fff",
                boxShadow: "0 1px 3px rgba(0,0,0,.15)",
                display: "grid",
                placeItems: "center",
                outline: active ? "2px solid #22d3ee" : "none",
                transform: active ? "scale(1.06)" : "none",
                transition: "transform .08s ease, outline .08s ease",
                overflow: "hidden",
            }}
            onMouseEnter={() => setHoveredTile?.(raw)}
            onMouseLeave={() => setHoveredTile?.(null)}
            onClick={() => setHoveredTile?.(raw)}
            title={norm}
        >
            {isLaizi ? (
                <img
                    src={LAIZI_SRC}
                    alt="bd"
                    draggable={false}
                    style={{ width: "100%", height: "100%", objectFit: "contain", display: "block" }}
                />
            ) : (
                <canvas ref={canvasRef} />
            )}

            {dim && (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "rgba(0,0,0,.35)",
                    }}
                />
            )}
        </div>
    );
}