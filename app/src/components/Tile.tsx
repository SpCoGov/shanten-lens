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

export default function Tile({
    tile,
    hoveredTile,
    setHoveredTile,
    width = 64,
    height = 84,
    dim = false,
    sideways = false,
    shadow = true,
}: {
    tile: string;
    hoveredTile?: string | null;
    setHoveredTile?: (t: string | null) => void;
    tianDoraTiles?: string[];
    doraCountByTile?: ReadonlyMap<string, number>;
    width?: number;
    height?: number;
    dim?: boolean;
    sideways?: boolean;
    shadow?: boolean;
}) {
    const tileSkin = useTileSkin();
    const raw = tile?.trim() ?? "";
    const isLaizi = raw === "bd";
    const norm = isLaizi ? "5m" : normalize(raw);
    const atlas = TILE_ATLAS_DEFAULT;
    const crop = atlas[norm] || atlas["5m"];
    const tempaiTileSrc = `/assets/mahjong/tempai-svg/${norm}.svg`;
    const active = hoveredTile ? isEquivalent(hoveredTile, raw) : false;
    const tileBoxShadow = shadow ? "var(--tile-shadow)" : "none";
    const displayWidth = sideways ? height : width;
    const displayHeight = sideways ? width : height;
    const atlasScaleX = width / crop.w;
    const atlasScaleY = height / crop.h;

    return (
        <div
            className={`mj-tile ${active ? "mj-tile--highlight" : ""} ${isLaizi ? "mj-tile--laizi" : ""}`}
            style={{
                position: "relative",
                width: displayWidth,
                height: displayHeight,
                borderRadius: 8,
                boxShadow: tileBoxShadow,
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
            <div style={{
                position: "absolute",
                left: "50%",
                top: "50%",
                width,
                height,
                overflow: "hidden",
                borderRadius: 8,
                background: tileSkin === "tempai-svg" ? "var(--tile-tempai-bg)" : "var(--tile-bg)",
                transform: `translate(-50%, -50%) rotate(${sideways ? 90 : 0}deg)`,
            }}>
                {isLaizi ? (
                    tileSkin === "tempai-svg" ? (
                        <div className="mj-tile__laizi-gradient" />
                    ) : (
                        <img
                            src={LAIZI_CLASSIC_SRC}
                            alt="bd"
                            draggable={false}
                            style={{width, height, objectFit: "fill", display: "block"}}
                        />
                    )
                ) : tileSkin === "tempai-svg" ? (
                    <img
                        src={tempaiTileSrc}
                        alt={raw || norm}
                        draggable={false}
                        style={{width, height, objectFit: "fill", display: "block", position: "relative", zIndex: 0}}
                    />
                ) : (
                    <img
                        src={ATLAS_CLASSIC_SRC}
                        alt={raw || norm}
                        draggable={false}
                        style={{
                            position: "absolute",
                            left: -crop.x * atlasScaleX,
                            top: -crop.y * atlasScaleY,
                            width: 400 * atlasScaleX,
                            height: 400 * atlasScaleY,
                            maxWidth: "none",
                        }}
                    />
                )}
            </div>

            {dim ? (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "var(--tile-dim-scrim)",
                        zIndex: 2,
                    }}
                />
            ) : null}
        </div>
    );
}
