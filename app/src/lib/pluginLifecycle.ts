export function safelyCleanup(cleanup?: () => void) {
    try { cleanup?.(); } catch (error) { console.error("Plugin cleanup failed", error); }
}

export function withPluginDeadline<T>(work: Promise<T>, signal: AbortSignal, message: string, timeoutMs = 5000): Promise<T> {
    return new Promise((resolve, reject) => {
        const finish = () => {
            clearTimeout(timer);
            signal.removeEventListener("abort", abort);
        };
        const abort = () => { finish(); reject(new Error(message)); };
        const timer = setTimeout(abort, timeoutMs);
        signal.addEventListener("abort", abort, {once: true});
        if (signal.aborted) abort();
        work.then((value) => { finish(); resolve(value); }, (error) => { finish(); reject(error); });
    });
}
