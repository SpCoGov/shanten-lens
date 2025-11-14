import { create } from "zustand";

export type LogLevel = "INFO" | "WARN" | "ERROR" | "STDOUT" | "STDERR";
export type LogItem = { ts: string; level: LogLevel; msg: string };
export type FrameItem = { ts: string; dir: "in" | "out"; raw: string };

const MAX_LOGS = 2000;
const MAX_FRAMES = 2000;

function now() {
    const d = new Date();
    return (
        [d.getHours(), d.getMinutes(), d.getSeconds()]
            .map(n => n.toString().padStart(2, "0"))
            .join(":") + "." + d.getMilliseconds().toString().padStart(3, "0")
    );
}

type LogState = {
    logs: LogItem[];
    frames: FrameItem[];
    addLog: (level: LogLevel, msg: string) => void;
    addFrame: (dir: "in" | "out", raw: string) => void;
    clearLogs: () => void;
    clearFrames: () => void;
};

export const useLogStore = create<LogState>((set, get) => ({
    logs: [],
    frames: [],
    addLog: (level, msg) => {
        const item: LogItem = { ts: now(), level, msg };
        const next = [...get().logs, item].slice(-MAX_LOGS);
        set({ logs: next });
        // 同步到控制台
        if (level === "ERROR" || level === "STDERR") console.error(`[${item.ts}] [${level}] ${msg}`);
        else if (level === "WARN") console.warn(`[${item.ts}] [${level}] ${msg}`);
        else console.log(`[${item.ts}] [${level}] ${msg}`);
    },
    addFrame: (dir, raw) => {
        try {
            if (raw.length <= 128 && raw[0] === "{") {
                const obj = JSON.parse(raw);
                if (obj && obj.type === "keep_alive") return;
            }
        } catch {
        }
        const item: FrameItem = { ts: now(), dir, raw };
        const next = [...get().frames, item].slice(-MAX_FRAMES);
        set({ frames: next });
    },
    clearLogs: () => set({ logs: [] }),
    clearFrames: () => set({ frames: [] }),
}));