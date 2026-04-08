import * as React from "react";

export type TileSkin = "classic" | "tempai-svg";

const TILE_SKIN_KEY = "sl-tile-skin";

function isTileSkin(value: string | null): value is TileSkin {
    return value === "classic" || value === "tempai-svg";
}

export function readTileSkin(): TileSkin {
    const saved = localStorage.getItem(TILE_SKIN_KEY);
    return isTileSkin(saved) ? saved : "tempai-svg";
}

export function setTileSkin(next: TileSkin) {
    localStorage.setItem(TILE_SKIN_KEY, next);
    window.dispatchEvent(new CustomEvent("sl:tile-skin-change", {detail: next}));
}

function subscribe(onStoreChange: () => void) {
    const onStorage = (e: StorageEvent) => {
        if (e.key === TILE_SKIN_KEY) onStoreChange();
    };
    const onCustom = () => onStoreChange();
    window.addEventListener("storage", onStorage);
    window.addEventListener("sl:tile-skin-change", onCustom);
    return () => {
        window.removeEventListener("storage", onStorage);
        window.removeEventListener("sl:tile-skin-change", onCustom);
    };
}

export function useTileSkin(): TileSkin {
    return React.useSyncExternalStore(subscribe, readTileSkin, readTileSkin);
}
