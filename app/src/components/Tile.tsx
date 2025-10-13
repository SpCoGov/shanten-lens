import React from "react";
import { useCroppedAtlasDefault, TileCode } from "./useCroppedAtlasDefault";

const LAIZI_SRC = "/assets/mjp_laizi.png";
const ALL = new Set<TileCode>([
    "0m","1m","2m","3m","4m","5m","6m","7m","8m","9m",
    "0p","1p","2p","3p","4p","5p","6p","7p","8p","9p",
    "0s","1s","2s","3s","4s","5s","6s","7s","8s","9s",
    "1z","2z","3z","4z","5z","6z","7z","bd",
]);
const norm = (t: string): TileCode => {
    const v = t === "0pm" ? "0m" : t;
    return (ALL.has(v as TileCode) ? (v as TileCode) : "5m");
};

export default function Tile({
                                 tile,
                                 hoveredTile,
                                 setHoveredTile,
                                 width = 64,
                                 height = 84,
                                 atlasSrc = "/assets/mjp_default.png",
                             }: {
    tile: string;
    hoveredTile?: string | null;
    setHoveredTile?: (t: string | null) => void;
    width?: number;
    height?: number;
    atlasSrc?: string;
}) {
    const key = norm(tile);
    const isLaizi = key === "bd";
    const { urls } = useCroppedAtlasDefault(atlasSrc);

    const isHovered = hoveredTile && norm(hoveredTile) === key;
    const src = isLaizi ? LAIZI_SRC : urls?.[key];

    return (
        <div
            style={{
                width, height,
                borderRadius: 8,
                background: "#fff",
                boxShadow: "0 1px 3px rgba(0,0,0,.15)",
                display: "grid",
                placeItems: "center",
                outline: isHovered ? "2px solid #22d3ee" : "none",
                transform: isHovered ? "scale(1.06)" : "none",
                transition: "transform .08s ease, outline .08s ease",
                overflow: "hidden",
            }}
            onMouseEnter={() => setHoveredTile?.(key)}
            onMouseLeave={() => setHoveredTile?.(null)}
        >
            {src ? (
                <img
                    src={src}
                    alt={key}
                    draggable={false}
                    style={{
                        display: "block",
                        width: "100%",
                        height: "100%",
                        objectFit: isLaizi ? "contain" : "cover",
                    }}
                />
            ) : (
                <div style={{ fontSize: 10, color: "#999" }}>…</div>
            )}
        </div>
    );
}