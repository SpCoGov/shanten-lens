import { useEffect, useMemo, useState } from "react";

export type TileCode =
    | "0m"|"1m"|"2m"|"3m"|"4m"|"5m"|"6m"|"7m"|"8m"|"9m"
    | "0p"|"1p"|"2p"|"3p"|"4p"|"5p"|"6p"|"7p"|"8p"|"9p"
    | "0s"|"1s"|"2s"|"3s"|"4s"|"5s"|"6s"|"7s"|"8s"|"9s"
    | "1z"|"2z"|"3z"|"4z"|"5z"|"6z"|"7z"
    | "bd";

type URLMap = Partial<Record<TileCode, string>>;

const ROWS: TileCode[][] = [
    ["0m","1m","2m","3m","4m","5m","6m"],
    ["7m","8m","9m","0p","1p","2p","3p"],
    ["4p","5p","6p","7p","8p","9p","0s"],
    ["1s","2s","3s","4s","5s","6s","7s"],
    ["8s","9s","1z","2z","3z","4z","5z"],
    ["6z","7z"],
];

const SPRITE_W = 400;
const SPRITE_H = 400;

const TILE_W = 50;
const TILE_H = 60;
const GAP_X  = 6;
const GAP_Y  = 6;
const LEFT_PAD   = 7; // (400 - (7*50 + 6*6)) / 2
const RIGHT_PAD  = 7;
const TOP_PAD    = 5; // (400 - (6*60 + 5*6)) / 2
const BOTTOM_PAD = 5;

// 去掉 1px 描边；若出现吃画面，改回 0
const INSET_X = 1;
const INSET_Y = 1;

export function useCroppedAtlasDefault(spriteSrc: string) {
    const [urls, setUrls] = useState<URLMap | null>(null);
    const [baseSize, setBaseSize] = useState<{ w: number; h: number } | null>(null);

    useEffect(() => {
        let cancelled = false;
        const img = new Image();
        img.onload = () => {
            if (cancelled) return;

            // 保护：尺寸不对就直接放弃（避免错图导致奇怪截取）
            if (img.naturalWidth !== SPRITE_W || img.naturalHeight !== SPRITE_H) {
                console.warn("mjp_default.png 不是 400×400，当前：",
                    img.naturalWidth, img.naturalHeight);
            }

            const usableW = SPRITE_W - LEFT_PAD - RIGHT_PAD;
            const usableH = SPRITE_H - TOP_PAD - BOTTOM_PAD;

            const rowsHeight = ROWS.length * TILE_H + (ROWS.length - 1) * GAP_Y;
            const startY = TOP_PAD + Math.max(0, Math.floor((usableH - rowsHeight) / 2));

            const canvas = document.createElement("canvas");
            const ctx = canvas.getContext("2d")!;
            const out: URLMap = {};

            const cut = (sx: number, sy: number, sw: number, sh: number) => {
                const x = Math.max(0, sx + INSET_X);
                const y = Math.max(0, sy + INSET_Y);
                const w = Math.max(1, sw - INSET_X * 2);
                const h = Math.max(1, sh - INSET_Y * 2);
                canvas.width = w;
                canvas.height = h;
                ctx.clearRect(0, 0, w, h);
                ctx.drawImage(img, x, y, w, h, 0, 0, w, h);
                return canvas.toDataURL("image/png");
            };

            let sy = startY;
            for (const row of ROWS) {
                const rowWidth = row.length * TILE_W + (row.length - 1) * GAP_X;
                const sx0 = LEFT_PAD + Math.max(0, Math.floor((usableW - rowWidth) / 2));

                let sx = sx0;
                for (const code of row) {
                    out[code] = cut(sx, sy, TILE_W, TILE_H);
                    sx += TILE_W + GAP_X;
                }
                sy += TILE_H + GAP_Y;
            }

            const probe = new Image();
            probe.onload = () => setBaseSize({ w: probe.width, h: probe.height });
            probe.src = out["1m"] || out[ROWS[0][1]]!;
            setUrls(out);
        };
        img.src = spriteSrc;
        return () => { cancelled = true; };
    }, [spriteSrc]);

    return useMemo(() => ({ urls, baseSize }), [urls, baseSize]);
}