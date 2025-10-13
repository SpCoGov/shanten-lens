import React from "react";
import { TILE_ATLAS_DEFAULT, TileCode } from "./useCroppedAtlasDefault";

const ATLAS_SRC = "/assets/mjp_default.png";
const LAIZI_SRC = "/assets/mjp_laizi.png";

// 将可能出现的别名/异常值归一化
function normalize(code: string): TileCode {
    const s = code.trim();
    if (s === "0pm") return "0m"; // 运行时统一视为 0m
    if (s === "bd") return "bd";
    const ok = [
        "0m","1m","2m","3m","4m","5m","6m","7m","8m","9m",
        "0p","1p","2p","3p","4p","5p","6p","7p","8p","9p",
        "0s","1s","2s","3s","4s","5s","6s","7s","8s","9s",
        "1z","2z","3z","4z","5z","6z","7z",
    ] as const;
    return (ok as readonly string[]).includes(s) ? (s as TileCode) : "5m";
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
                             }: {
    tile: string;
    hoveredTile?: string | null;
    setHoveredTile?: (t: string | null) => void;
    width?: number;
    height?: number;
}) {
    const norm = normalize(tile);
    const isLaizi = norm === "bd";
    const canvasRef = React.useRef<HTMLCanvasElement | null>(null);

    const isHovered = hoveredTile && normalize(hoveredTile) === norm;

    React.useEffect(() => {
        if (isLaizi) return; // 癞子直接用 <img>
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
            style={{
                width,
                height,
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
            onMouseEnter={() => setHoveredTile?.(norm)}
            onMouseLeave={() => setHoveredTile?.(null)}
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
        </div>
    );
}