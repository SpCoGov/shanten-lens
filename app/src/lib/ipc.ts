import {listen, type UnlistenFn} from "@tauri-apps/api/event";
import {
    commands,
    type AmuletActionRequest,
    type BackendSnapshot,
    type CommandResult,
    type FlowDumpResult,
    type JsonValue,
    type PacketLogItem,
    type PacketLogSnapshot,
    type PipelineConfig,
    type SwitchAction,
    type SwitchRequest,
    type TsumoLoopStatus,
    type VersionMismatch,
} from "../bindings";
import {useLogStore, type BackendLogEntry} from "./logStore";
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
    PacketLogItem,
    PacketLogSnapshot,
    PipelineConfig,
    SwitchAction,
    SwitchRequest,
    TsumoLoopStatus,
    VersionMismatch,
};

export type BackendEventMap = {
    backend_log: BackendLogEntry;
    update_registry: RegistryPayload;
    update_fuse_config: FuseConfig;
    update_autorun_config: AutoRunnerConfig;
    update_config: ConfigTables;
    update_gamestate: GameStateData;
    autorun_status: AutoRunnerStatus;
    tsumo_loop_status: TsumoLoopStatus;
    packet_log_event: PacketLogItem;
    packet_pipeline: PipelineConfig;
    discard_recommendation: unknown;
    souzu_switch_execution: unknown;
    msgbox: {id: string; title?: string; message: string; okText?: string; cancelText?: string; values?: Record<string, JsonValue>};
    ui_toast: {msg?: string; msg_key?: string; msg_values?: Record<string, JsonValue>; kind?: "info" | "success" | "error"; duration?: number};
};

export function onBackendEvent<K extends keyof BackendEventMap>(
    event: K,
    handler: (payload: BackendEventMap[K]) => void,
): Promise<UnlistenFn> {
    return listen<BackendEventMap[K]>(`backend:${event}`, ({payload}) => handler(payload));
}

export function subscribeBackendEvent<K extends keyof BackendEventMap>(
    event: K,
    handler: (payload: BackendEventMap[K]) => void,
): UnlistenFn {
    let active = true;
    let unlisten: UnlistenFn | undefined;
    void onBackendEvent(event, handler).then((value) => {
        if (active) unlisten = value;
        else value();
    });
    return () => {
        active = false;
        unlisten?.();
    };
}

export const getSnapshot = commands.backendSnapshot;
export const checkVersion = () => commands.backendCheckVersion(APP_VERSION);
export const getPacketPipeline = commands.backendGetPacketPipeline;
export const setPacketPipeline = commands.backendSetPacketPipeline;
export const getPacketLog = commands.backendGetPacketLog;
export const replayPacket = commands.backendReplayPacket;
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

let coreListeners: Promise<UnlistenFn[]> | null = null;

export function initializeBackend() {
    if (!coreListeners) {
        coreListeners = Promise.all([
            onBackendEvent("backend_log", (entry) => useLogStore.getState().addBackendLogs([entry])),
            onBackendEvent("update_registry", setRegistry),
            onBackendEvent("update_fuse_config", setFuseConfig),
            onBackendEvent("update_autorun_config", setAutoConfig),
            onBackendEvent("autorun_status", setAutoStatus),
            onBackendEvent("ui_toast", (data) => {
                const text = translateToastMessage(data);
                if (text) pushToast(text, data.kind ?? "info", data.duration ?? 2200);
            }),
        ]);
    }
    return getSnapshot().then((snapshot): InitializedBackendSnapshot => {
        useLogStore.getState().setBackendSnapshot(snapshot.backendLogs);
        const registry = snapshot.registry as RegistryPayload;
        const fuseConfig = snapshot.fuseConfig as FuseConfig;
        const autorunConfig = snapshot.autorunConfig as AutoRunnerConfig;
        const autorunStatus = snapshot.autorunStatus as AutoRunnerStatus;
        setRegistry(registry);
        setFuseConfig(fuseConfig);
        setAutoConfig(autorunConfig);
        setAutoStatus(autorunStatus);
        return {
            ...snapshot,
            registry,
            fuseConfig,
            autorunConfig,
            autorunStatus,
            config: snapshot.config as ConfigTables,
            gameState: snapshot.gameState as unknown as GameStateData,
        };
    });
}
