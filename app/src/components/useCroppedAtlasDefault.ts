export interface TileCrop {
    x: number;
    y: number;
    w: number;
    h: number;
}

// 供 UI 组件使用的 tile 代码集合（含 bd）
export type TileCode =
    | "0m" | "1m" | "2m" | "3m" | "4m" | "5m" | "6m" | "7m" | "8m" | "9m"
    | "0p" | "1p" | "2p" | "3p" | "4p" | "5p" | "6p" | "7p" | "8p" | "9p"
    | "0s" | "1s" | "2s" | "3s" | "4s" | "5s" | "6s" | "7s" | "8s" | "9s"
    | "1z" | "2z" | "3z" | "4z" | "5z" | "6z" | "7z"
    | "bd";

export const TILE_ATLAS_DEFAULT: Record<string, TileCrop> = (() => {
    const w = 41;
    const h = 56.5;

    // 图上从 (0,0) 开始紧贴排列，无间隙
    const order = [
        // 第1行
        "0m","1m","2m","3m","4m","5m","6m",
        // 第2行
        "7m","8m","9m","0p","1p","2p","3p",
        // 第3行
        "4p","5p","6p","7p","8p","9p","0s",
        // 第4行
        "1s","2s","3s","4s","5s","6s","7s",
        // 第5行
        "8s","9s","1z","2z","3z","4z","5z",
        // 第6行（2张）
        "6z","7z",
    ];

    const atlas: Record<string, TileCrop> = {};
    let i = 0;
    for (let r = 0; r < 6; r++) {
        for (let c = 0; c < 7; c++) {
            if (r === 5 && c > 1) break; // 第6行只有2张
            const x = c * w;
            const y = r * h;
            atlas[order[i]] = { x, y, w, h };
            i++;
        }
    }

    return atlas;
})();