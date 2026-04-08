import React from "react";
import "../styles/theme.css";
import {TILE_ATLAS_DEFAULT, type TileCode} from "./useCroppedAtlasDefault";
import {useTileSkin} from "../lib/tileSkin";

const ATLAS_CLASSIC_SRC = "/assets/mjp_default.png";
const LAIZI_CLASSIC_SRC = "/assets/mjp_laizi.png";

function normalize(code: string): TileCode {
    const s = code.trim();
    const ok = [
        "0m", "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
        "0p", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p",
        "0s", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s",
        "1z", "2z", "3z", "4z", "5z", "6z", "7z",
    ] as const;
    return (ok as readonly string[]).includes(s) ? (s as TileCode) : "5m";
}

function normalizeLoose(raw: string): string {
    const s = raw?.trim() ?? "";
    if (/^[0-9][mps]$/.test(s)) return s;
    if (/^[mps][0-9]$/.test(s)) return `${s[1]}${s[0]}`;
    if (/^[1-7]z$/.test(s)) return s;
    if (/^z[1-7]$/.test(s)) return `${s[1]}z`;
    return s;
}

function parseSuitVal(n: string): { suit: string | null; val: number | null } {
    if (/^[0-9][mps]$/.test(n)) return {suit: n[1], val: Number(n[0])};
    if (/^[1-7]z$/.test(n)) return {suit: "z", val: Number(n[0])};
    return {suit: null, val: null};
}

function isEquivalent(aRaw: string, bRaw: string): boolean {
    const a = normalizeLoose(aRaw);
    const b = normalizeLoose(bRaw);
    if (a === b) return true;

    const pa = parseSuitVal(a);
    const pb = parseSuitVal(b);
    if (!pa.suit || !pb.suit) return false;
    if (pa.suit !== pb.suit) return false;
    if (pa.suit === "z") return a === b;
    if (pa.val === pb.val) return true;
    return new Set([pa.val, pb.val]).has(0) && new Set([pa.val, pb.val]).has(5);
}

type AtlasCache = { img?: HTMLImageElement; ready: boolean; cbs: Array<() => void> };

const atlasCache = new Map<string, AtlasCache>();

function getCache(src: string): AtlasCache {
    let cache = atlasCache.get(src);
    if (!cache) {
        cache = {img: undefined, ready: false, cbs: []};
        atlasCache.set(src, cache);
    }
    return cache;
}

function loadAtlas(src: string, onReady: () => void) {
    const cache = getCache(src);
    if (cache.ready) return onReady();
    cache.cbs.push(onReady);
    if (!cache.img) {
        const img = new Image();
        img.src = src;
        img.onload = () => {
            cache.ready = true;
            cache.cbs.splice(0).forEach((fn) => fn());
        };
        cache.img = img;
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
    const tileSkin = useTileSkin();
    const raw = tile?.trim() ?? "";
    const isLaizi = raw === "bd";
    const norm = isLaizi ? "5m" : normalize(raw);
    const canvasRef = React.useRef<HTMLCanvasElement | null>(null);

    const atlas = TILE_ATLAS_DEFAULT;
    const atlasSrc = ATLAS_CLASSIC_SRC;
    const tempaiTileSrc = `/assets/mahjong/tempai-svg/${norm}.svg`;
    const active = hoveredTile ? isEquivalent(hoveredTile, raw) : false;
    const tileBoxShadow = tileSkin === "tempai-svg"
        ? "var(--tile-shadow), inset -8px 0 0 rgba(204, 204, 204, .82), inset -1px 0 0 rgba(115, 115, 115, .34)"
        : "var(--tile-shadow)";

    React.useEffect(() => {
        if (isLaizi || tileSkin === "tempai-svg") return;
        const crop = atlas[norm] || atlas["5m"];
        const draw = () => {
            const cvs = canvasRef.current;
            const img = getCache(atlasSrc).img;
            if (!cvs || !img) return;
            cvs.width = width;
            cvs.height = height;
            const ctx = cvs.getContext("2d");
            if (!ctx) return;
            ctx.clearRect(0, 0, width, height);
            ctx.imageSmoothingEnabled = true;
            ctx.imageSmoothingQuality = "high";
            ctx.drawImage(img, crop.x, crop.y, crop.w, crop.h, 0, 0, width, height);
        };
        loadAtlas(atlasSrc, draw);
        if (getCache(atlasSrc).ready) draw();
    }, [atlas, atlasSrc, height, isLaizi, norm, tileSkin, width]);

    return (
        <div
            className={`mj-tile ${active ? "mj-tile--highlight" : ""} ${isLaizi ? "mj-tile--laizi" : ""}`}
            style={{
                position: "relative",
                width,
                height,
                borderRadius: 8,
                background: tileSkin === "tempai-svg" ? "#f4f4f4" : "var(--tile-bg)",
                boxShadow: tileBoxShadow,
                display: "grid",
                placeItems: "center",
                outline: active ? "var(--tile-outline)" : "none",
                transform: active ? "scale(1.06)" : "none",
                transition: "transform .08s ease, outline .08s ease",
                overflow: "hidden",
            }}
            onMouseEnter={() => setHoveredTile?.(raw)}
            onMouseLeave={() => setHoveredTile?.(null)}
            onClick={() => setHoveredTile?.(raw)}
            title={raw || norm}
        >
            {isLaizi ? (
                tileSkin === "tempai-svg" ? (
                    <div className="mj-tile__laizi-gradient" />
                ) : (
                    <img
                        src={LAIZI_CLASSIC_SRC}
                        alt="bd"
                        draggable={false}
                        style={{width: "100%", height: "100%", objectFit: "contain", display: "block"}}
                    />
                )
            ) : tileSkin === "tempai-svg" ? (
                <img
                    src={tempaiTileSrc}
                    alt={raw || norm}
                    draggable={false}
                    style={{width: "100%", height: "100%", objectFit: "fill", display: "block"}}
                />
            ) : (
                <canvas ref={canvasRef} />
            )}

            {dim ? (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "var(--tile-dim-scrim)",
                    }}
                />
            ) : null}
        </div>
    );
}
