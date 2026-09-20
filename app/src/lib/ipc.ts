import {listen, type UnlistenFn} from "@tauri-apps/api/event";
import {
    commands,
    type AmuletActionRequest,
    type BackendSnapshot,
    type CommandResult,
    type FlowDumpResult,
    type JsonValue,
    type MarketplaceInstallBlock,
    type MarketplacePluginInfo,
    type MarketplacePluginStatus,
    type MarketplaceSnapshot,
    type MarketplaceSourceInfo,
    type PacketLogItem,
    type PacketLogSnapshot,
    type PacketModuleInfo,
    type PacketOperation,
    type PacketSubscription,
    type PipelineConfig,
    type PluginInfo,
    type PluginScanError,
    type PluginFrontendBundle,
    type PluginUpdateInfo,
    type SwitchAction,
    type SwitchRequest,
    type TsumoLoopStatus,
    type VersionMismatch,
} from "../bindings";
import {useLogStore} from "./logStore";
import {setRegistry, type RegistryPayload} from "./registryStore";
import {setFuseConfig, type FuseConfig} from "./fuseStore";
import {type AutoRunnerConfig, type AutoRunnerStatus, setAutoConfig, setAutoStatus} from "./autoRunnerStore";
import {pushToast} from "./toast";
import {APP_VERSION} from "./version";
import type {GameStateData} from "./gamestate";
import {t} from "i18next";

export type ConfigTables = Partial<Record<string, Partial<Record<string, JsonValue>>>>;
export type PipelineModule = PipelineConfig["modules"][number];
export type InitializedBackendSnapshot = Omit<BackendSnapshot,
    "fuseConfig" | "autorunConfig" | "registry" | "config" | "gameState" | "autorunStatus" | "packetPipeline"
> & {
    fuseConfig: FuseConfig;
    autorunConfig: AutoRunnerConfig;
    registry: RegistryPayload;
    config: ConfigTables;
    gameState: GameStateData;
    autorunStatus: AutoRunnerStatus;
    packetPipeline: PipelineConfig;
};
export type {
    AmuletActionRequest,
    BackendSnapshot,
    CommandResult,
    FlowDumpResult,
    JsonValue,
    MarketplaceInstallBlock,
    MarketplacePluginInfo,
    MarketplacePluginStatus,
    MarketplaceSnapshot,
    MarketplaceSourceInfo,
    PacketLogItem,
    PacketLogSnapshot,
    PacketModuleInfo,
    PacketOperation,
    PacketSubscription,
    PipelineConfig,
    PluginInfo,
    PluginScanError,
    PluginFrontendBundle,
    PluginUpdateInfo,
    SwitchAction,
    SwitchRequest,
    TsumoLoopStatus,
    VersionMismatch,
};

export type BackendEventMap = {
    backend_log: BackendSnapshot["backendLogs"][number];
    proxy_status: {running: boolean; error: string | null};
    resync_required: null;
    souzu_switch_control_result: {action: string; ok: boolean; reason?: string};
    update_registry: RegistryPayload;
    update_fuse_config: FuseConfig;
    update_autorun_config: AutoRunnerConfig;
    update_config: ConfigTables;
    update_gamestate: GameStateData;
    update_game_record: Record<string, JsonValue>;
    game_record_override_status: boolean;
    autorun_status: AutoRunnerStatus;
    tsumo_loop_status: TsumoLoopStatus;
    packet_log_event: PacketLogItem;
    packet_pipeline: PipelineConfig;
    packet_modules: PacketModuleInfo[];
    plugin_status: PluginInfo[];
    plugin_update_available: PluginUpdateInfo;
    discard_recommendation: unknown;
    souzu_switch_execution: unknown;
    msgbox: {id: string; title?: string; message: string; okText?: string; cancelText?: string; values?: Record<string, JsonValue>};
    ui_toast: {msg?: string; msg_key?: string; msg_values?: Record<string, JsonValue>; kind?: "info" | "success" | "error"; duration?: number};
};

let eventVersion = 0;
const latest = new Map<keyof BackendEventMap, {version: number; payload: unknown}>();
const handlers = new Map<keyof BackendEventMap, Set<(payload: any) => void>>();

export function onBackendEvent<K extends keyof BackendEventMap>(
    event: K,
    handler: (payload: BackendEventMap[K]) => void,
): Promise<UnlistenFn> {
    const callbacks = handlers.get(event) ?? new Set();
    handlers.set(event, callbacks);
    callbacks.add(handler);
    return listen<BackendEventMap[K]>(`backend:${event}`, ({payload}) => {
        latest.set(event, {version: ++eventVersion, payload});
        handler(payload);
    }).then((unlisten) => () => { callbacks.delete(handler); unlisten(); }, (error) => {
        callbacks.delete(handler);
        throw error;
    });
}

export function subscribeBackendEvent<K extends keyof BackendEventMap>(
    event: K,
    handler: (payload: BackendEventMap[K]) => void,
): UnlistenFn {
    let active = true;
    let unlisten: UnlistenFn | undefined;
    void onBackendEvent(event, (payload) => { if (active) handler(payload); }).then((value) => {
        if (active) unlisten = value;
        else value();
    }).catch((error) => useLogStore.getState().addLog("ERROR", String(error)));
    return () => {
        active = false;
        unlisten?.();
    };
}

export const getSnapshot = commands.backendSnapshot;
export const checkVersion = () => commands.backendCheckVersion(APP_VERSION);
export const getPacketPipeline = commands.backendGetPacketPipeline;
export const getPacketModules = commands.backendGetPacketModules;
export const getPluginScanErrors = commands.backendGetPluginScanErrors;
export const reportPluginFrontendHealth = commands.backendReportPluginFrontendHealth;
export const installPlugin = commands.backendInstallPlugin;
export const uninstallPlugin = commands.backendUninstallPlugin;
export const getPlugins = commands.backendGetPlugins;
export const getPluginMarketplace = commands.backendGetPluginMarketplace;
export const addPluginMarketplaceSource = commands.backendAddPluginMarketplaceSource;
export const removePluginMarketplaceSource = commands.backendRemovePluginMarketplaceSource;
export const installMarketplacePlugin = commands.backendInstallMarketplacePlugin;
export const getPluginFrontends = commands.backendGetPluginFrontends;
export const getPluginConfig = commands.backendGetPluginConfig;
export const setPluginConfig = commands.backendSetPluginConfig;
export const invokePlugin = commands.backendInvokePlugin;
export const rescanPlugins = commands.backendRescanPlugins;
export const setPluginEnabled = commands.backendSetPluginEnabled;
export const restartPlugin = commands.backendRestartPlugin;
export const setPluginAutoUpdate = commands.backendSetPluginAutoUpdate;
export const checkPluginUpdates = commands.backendCheckPluginUpdates;
export const updatePlugin = commands.backendUpdatePlugin;
export const setPacketPipeline = commands.backendSetPacketPipeline;
export const getPacketLog = commands.backendGetPacketLog;
export const replayPacket = commands.backendReplayPacket;
export const fetchGameRecord = commands.backendFetchGameRecord;
export const overrideGameRecord = commands.backendOverrideGameRecord;
export const updateConfig = (config: ConfigTables) => commands.backendUpdateConfig(config);
export const setBackendLocale = commands.backendSetLocale;
export const dumpFlows = commands.backendDumpFlows;
export const fetchActivity = (activityId = 260511) => commands.backendFetchActivity(activityId);
export const discardTile = commands.backendDiscardTile;
export const upgradeShopBuff = (id: number, activityId = 260511) => commands.backendUpgradeShopBuff(activityId, id);
export const runAmuletAction = commands.backendAmuletAction;
export const startTsumoLoop = (intervalMs: number, resetCount = false) => commands.backendStartTsumoLoop(intervalMs, resetCount);
export const stopTsumoLoop = commands.backendStopTsumoLoop;
export const runAutorun = (action: "start" | "stop" | "step" | "probe" | "notify_test_email" | "set_mode", options: {force?: boolean; mode?: "continuous" | "step"} = {}) =>
    commands.backendAutorun(action, options.force ?? false, options.mode ?? null);
export const resolveConfirmation = commands.backendResolveConfirmation;
export const runSwitch = commands.backendSwitch;
export const openConfigDir = commands.backendOpenConfigDir;
export const openLogDir = commands.backendOpenLogDir;
export const openPluginDir = commands.backendOpenPluginDir;

function translateToastMessage(data: BackendEventMap["ui_toast"]) {
    const key = String(data.msg_key ?? "");
    if (!key) return String(data.msg ?? "");
    const values = {...(data.msg_values ?? {})};
    if (values.ssl === "on" || values.ssl === "off") values.ssl = t(`autorun.email_error.ssl_${values.ssl}`);
    if (values.detail) values.detail = t("autorun.email_error.detail", {detail: values.detail});
    if (typeof values.reason_key === "string") {
        const reasonValues = typeof values.reason_values === "object" && values.reason_values
            ? {...values.reason_values as Record<string, JsonValue>}
            : {};
        if (reasonValues.ssl === "on" || reasonValues.ssl === "off") {
            reasonValues.ssl = t(`autorun.email_error.ssl_${reasonValues.ssl}`);
        }
        if (reasonValues.detail) reasonValues.detail = t("autorun.email_error.detail", {detail: reasonValues.detail});
        values.reason = String(t(values.reason_key, reasonValues));
    }
    return String(t(key, values));
}

const snapshotEvents = {
    registry: "update_registry", fuseConfig: "update_fuse_config",
    autorunConfig: "update_autorun_config", autorunStatus: "autorun_status",
    config: "update_config", gameState: "update_gamestate",
    tsumoLoopStatus: "tsumo_loop_status", packetPipeline: "packet_pipeline", proxyStatus: "proxy_status",
} as const;
let coreListeners: Promise<UnlistenFn[]> | null = null;

let snapshotRequest: Promise<InitializedBackendSnapshot> | null = null;
function synchronizedSnapshot(): Promise<InitializedBackendSnapshot> {
    snapshotRequest ??= readSynchronizedSnapshot().finally(() => { snapshotRequest = null; });
    return snapshotRequest;
}

async function readSynchronizedSnapshot(): Promise<InitializedBackendSnapshot> {
    const version = eventVersion;
    const logs: BackendSnapshot["backendLogs"] = [];
    const unlisten = await onBackendEvent("backend_log", (entry) => logs.push(entry));
    try {
        const snapshot = await getSnapshot();
        for (const [field, event] of Object.entries(snapshotEvents)) {
            const update = latest.get(event);
            if (update && update.version > version) (snapshot as any)[field] = update.payload;
        }
        const known = new Set(snapshot.backendLogs.map((entry) => JSON.stringify(entry)));
        snapshot.backendLogs.push(...logs.filter((entry) => !known.has(JSON.stringify(entry))).map((entry) => ({...entry, file: entry.file ?? null, line: entry.line ?? null})));
        useLogStore.getState().setBackendSnapshot(snapshot.backendLogs);
        return snapshot as unknown as InitializedBackendSnapshot;
    } finally { unlisten(); }
}

function applySnapshot(snapshot: InitializedBackendSnapshot) {
    for (const [field, event] of Object.entries(snapshotEvents)) {
        const payload = (snapshot as any)[field];
        handlers.get(event)?.forEach((handler) => handler(payload));
    }
}

export async function initializeBackend() {
    if (!coreListeners) {
        coreListeners = Promise.allSettled([
            onBackendEvent("backend_log", (entry) => useLogStore.getState().addBackendLogs([entry])),
            onBackendEvent("update_registry", setRegistry),
            onBackendEvent("update_fuse_config", setFuseConfig),
            onBackendEvent("update_autorun_config", setAutoConfig),
            onBackendEvent("autorun_status", setAutoStatus),
            ...(["update_config", "update_gamestate", "tsumo_loop_status", "packet_pipeline"] as const)
                .map((event) => onBackendEvent(event, () => {})),
            onBackendEvent("proxy_status", (status) => {
                if (status.error) pushToast(t("diagnostics.proxy_start_failed", {reason: status.error}), "error", 10000);
            }),
            onBackendEvent("souzu_switch_control_result", (result) => {
                if (!result.ok) pushToast(t("blackhole.control_failed", {reason: result.reason ?? ""}), "error", 5000);
            }),
            onBackendEvent("resync_required", () => {
                void synchronizedSnapshot().then(applySnapshot).catch((error) => useLogStore.getState().addLog("ERROR", String(error)));
            }),
            onBackendEvent("ui_toast", (data) => {
                const text = translateToastMessage(data);
                if (text) pushToast(text, data.kind ?? "info", data.duration ?? 2200);
            }),
        ]).then((results) => {
            const failure = results.find((result) => result.status === "rejected");
            const listeners = results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
            if (failure?.status === "rejected") {
                listeners.forEach((unlisten) => unlisten());
                coreListeners = null;
                throw failure.reason;
            }
            return listeners;
        });
    }
    await coreListeners;
    const snapshot = await synchronizedSnapshot();
    applySnapshot(snapshot);
    return snapshot;
}
