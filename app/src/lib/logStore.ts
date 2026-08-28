import {create} from "zustand";

export type LogLevel = "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" | "STDOUT" | "STDERR";
export type LogItem = {
    ts: string;
    ts_ms: number;
    level: LogLevel;
    target: string;
    msg: string;
    file?: string;
    line?: number;
    fields?: Record<string, unknown>;
};
export type BackendLogEntry = {
    ts_ms: number;
    level: string;
    target: string;
    message: string;
    file?: string | null;
    line?: number | null;
    fields?: Record<string, unknown>;
};
const MAX_ENTRIES = 1_000;
const MAX_LOG_MESSAGE_CHARS = 12_000;
const TRUNCATED_SUFFIX = "\n... [truncated in diagnostics view]";

function formatTime(timestamp: number) {
    const date = new Date(timestamp);
    return `${[date.getHours(), date.getMinutes(), date.getSeconds()]
        .map((value) => value.toString().padStart(2, "0"))
        .join(":")}.${date.getMilliseconds().toString().padStart(3, "0")}`;
}

function limitMessage(message: string) {
    return message.length <= MAX_LOG_MESSAGE_CHARS
        ? message
        : message.slice(0, MAX_LOG_MESSAGE_CHARS) + TRUNCATED_SUFFIX;
}

function localEntry(level: LogLevel, message: string): LogItem {
    const ts_ms = Date.now();
    return {ts: formatTime(ts_ms), ts_ms, level, target: "shanten_lens::frontend", msg: limitMessage(message)};
}

function backendEntry(entry: BackendLogEntry): LogItem | null {
    const ts_ms = Number(entry?.ts_ms);
    const level = String(entry?.level || "INFO").toUpperCase() as LogLevel;
    if (!Number.isFinite(ts_ms) || typeof entry?.message !== "string") return null;
    return {
        ts: formatTime(ts_ms),
        ts_ms,
        level,
        target: String(entry.target || "shanten_backend"),
        msg: limitMessage(entry.message),
        file: entry.file || undefined,
        line: entry.line ?? undefined,
        fields: entry.fields,
    };
}

function append(current: LogItem[], items: LogItem[]) {
    return current.concat(items).slice(-MAX_ENTRIES);
}

type LogState = {
    logs: LogItem[];
    addLog: (level: LogLevel, message: string) => void;
    addLogs: (level: LogLevel, messages: string[]) => void;
    addBackendLogs: (entries: BackendLogEntry[]) => void;
    setBackendSnapshot: (entries: BackendLogEntry[]) => void;
    clearLogs: () => void;
};

export const useLogStore = create<LogState>((set) => ({
    logs: [],
    addLog: (level, message) => set((state) => ({logs: append(state.logs, [localEntry(level, message)])})),
    addLogs: (level, messages) => {
        if (messages.length) set((state) => ({logs: append(state.logs, messages.map((message) => localEntry(level, message)))}));
    },
    addBackendLogs: (entries) => {
        const items = entries.map(backendEntry).filter((entry): entry is LogItem => entry !== null);
        if (items.length) set((state) => ({logs: append(state.logs, items)}));
    },
    setBackendSnapshot: (entries) => set({
        logs: entries.map(backendEntry).filter((entry): entry is LogItem => entry !== null).slice(-MAX_ENTRIES),
    }),
    clearLogs: () => set({logs: []}),
}));

export const LOG_BUFFER_CAP = MAX_ENTRIES;
