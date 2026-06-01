import { create } from "zustand";

export type LogLevel = "INFO" | "WARN" | "ERROR" | "STDOUT" | "STDERR";
export type LogItem = { ts: string; level: LogLevel; msg: string };
export type FrameItem = { ts: string; dir: "in" | "out"; raw: string };

const MAX_LOGS = 2000;
const MAX_FRAMES = 2000;
const MAX_LOG_MESSAGE_CHARS = 12000;
const TRUNCATED_SUFFIX = "\n... [truncated in diagnostics view]";

function now() {
    const d = new Date();
    return (
        [d.getHours(), d.getMinutes(), d.getSeconds()]
            .map((n) => n.toString().padStart(2, "0"))
            .join(":") + "." + d.getMilliseconds().toString().padStart(3, "0")
    );
}

type LogState = {
    logs: LogItem[];
    frames: FrameItem[];
    addLog: (level: LogLevel, msg: string) => void;
    addLogs: (level: LogLevel, msgs: string[]) => void;
    addFrame: (dir: "in" | "out", raw: string) => void;
    clearLogs: () => void;
    clearFrames: () => void;
};

function mirrorToConsole(item: LogItem) {
    if (item.level !== "ERROR") return;
    if (item.level === "ERROR") console.error(`[${item.ts}] [${item.level}] ${item.msg}`);
}

function limitMessage(msg: string) {
    if (msg.length <= MAX_LOG_MESSAGE_CHARS) return msg;
    return msg.slice(0, MAX_LOG_MESSAGE_CHARS) + TRUNCATED_SUFFIX;
}

export const useLogStore = create<LogState>((set, get) => ({
    logs: [],
    frames: [],
    addLog: (level, msg) => {
        const item: LogItem = { ts: now(), level, msg: limitMessage(msg) };
        const next = [...get().logs, item].slice(-MAX_LOGS);
        set({ logs: next });
        mirrorToConsole(item);
    },
    addLogs: (level, msgs) => {
        if (!msgs.length) return;
        const items = msgs.map((msg) => ({ ts: now(), level, msg: limitMessage(msg) } as LogItem));
        const next = [...get().logs, ...items].slice(-MAX_LOGS);
        set({ logs: next });
        for (const item of items) mirrorToConsole(item);
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
