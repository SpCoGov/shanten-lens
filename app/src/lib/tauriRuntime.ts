import {emit, listen, type EventCallback, type UnlistenFn} from "@tauri-apps/api/event";

function hasTauriInternals() {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function safeListen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
    if (!hasTauriInternals()) return () => {};
    try {
        return await listen<T>(event, handler);
    } catch {
        return () => {};
    }
}

export async function safeEmit<T>(event: string, payload?: T): Promise<void> {
    if (!hasTauriInternals()) return;
    try {
        await emit(event, payload);
    } catch {
    }
}
