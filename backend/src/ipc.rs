use crate::{logging::LogEntry, pipeline::PipelineConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::collections::HashMap;

pub type JsonMap = HashMap<String, Value>;
pub type ConfigTables = HashMap<String, JsonMap>;

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct FuseGuardItems {
    amulets: Vec<u64>,
    badges: Vec<u64>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct FuseConfig {
    guard_skip_contains: FuseGuardItems,
    enable_skip_guard: Option<bool>,
    enable_shop_force_pick: Option<bool>,
    enable_ting_ready_skip_guard: Option<bool>,
    enable_prestart_kavi_guard: Option<bool>,
    conduction_min_count: Option<u64>,
    enable_anti_steal_eat: Option<bool>,
    enable_missing_hand_tile_guard: Option<bool>,
    enable_kavi_plus_buffer_guard: Option<bool>,
    enable_hanabi_win_guard: Option<bool>,
    enable_exit_coin_guard: Option<bool>,
    enable_exit_life_guard: Option<bool>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct EmailNotifyConfig {
    enabled: bool,
    host: String,
    port: u16,
    ssl: bool,
    from: String,
    pass: String,
    to: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct AutoRunnerConfig {
    end_count: u64,
    targets: Vec<Value>,
    cutoff_level: Option<u64>,
    op_interval_ms: Option<u64>,
    need_pionner_badge_count: Option<u64>,
    record_detailed_operations: Option<bool>,
    email_notify: Option<EmailNotifyConfig>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct RegistryAmulet {
    id: u64,
    icon_id: u64,
    name: String,
    rarity: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct RegistryBadge {
    id: u64,
    icon_id: u64,
    name: String,
    rarity: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct RegistryPayload {
    amulets: Vec<RegistryAmulet>,
    badges: Vec<RegistryBadge>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct GameStateData {
    stage: i64,
    coin: String,
    point: Option<String>,
    target_point: Option<String>,
    level: Option<u64>,
    node: Option<u64>,
    map_nodes: Option<Vec<Value>>,
    deck_map: HashMap<String, String>,
    hand_tiles: Vec<u64>,
    dora_tiles: Vec<u64>,
    tian_dora_tiles: Option<Vec<Value>>,
    ming: Option<Vec<Value>>,
    replacement_tiles: Vec<u64>,
    wall_tiles: Vec<u64>,
    ended: Option<bool>,
    desktop_remain: u64,
    locked_tiles: Vec<u64>,
    switch_used_tiles: Vec<u64>,
    effect_list: Option<Vec<Value>>,
    goods: Option<Vec<Value>>,
    refresh_price: Option<u64>,
    candidate_effect_list: Option<Vec<Value>>,
    boss_buff: Option<Vec<u64>>,
    shop_buff_list: Option<HashMap<String, u64>>,
    change_tile_count: Option<u64>,
    total_change_tile_count: Option<u64>,
    max_effect_volume: Option<u64>,
    tile_score_map: Option<HashMap<String, String>>,
    fan_value_map: Option<HashMap<String, String>>,
    character_id: Option<Value>,
    hp: Option<Value>,
    max_hp: Option<Value>,
    update_reason: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, Type)]
pub struct AutoRunnerStatus {
    mode: Option<String>,
    running: bool,
    runs: u64,
    elapsed_ms: u64,
    best_achieved_count: u64,
    current_achieved_count: Option<u64>,
    current_step: Option<String>,
    last_error: Option<String>,
    started_at: Option<u64>,
    game_ready: Option<bool>,
    has_live_game: Option<bool>,
    game_ready_reason: Option<String>,
    game_ready_code: Option<String>,
    probe_fail_count: Option<u64>,
    preferred_flow_ready: Option<bool>,
    preferred_flow_peer: Option<String>,
    remake_records: Option<Vec<Value>>,
    best_remake_record: Option<Value>,
    record_detailed_operations: Option<bool>,
    operation_records: Option<Vec<Value>>,
    operation_count_by_run: Option<HashMap<String, u64>>,
}

#[derive(Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackendSnapshot {
    pub version: String,
    pub backend_logs: Vec<LogEntry>,
    pub packet_pipeline: PipelineConfig,
    pub fuse_config: FuseConfig,
    pub autorun_config: AutoRunnerConfig,
    pub registry: RegistryPayload,
    pub config: ConfigTables,
    pub game_state: GameStateData,
    pub autorun_status: AutoRunnerStatus,
    pub tsumo_loop_status: TsumoLoopStatus,
    pub packet_log: PacketLogSnapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct PacketLogItem {
    pub direction: crate::pipeline::Direction,
    #[serde(rename = "type")]
    pub packet_type: String,
    pub method: String,
    pub id: Option<u32>,
    pub data: Value,
    pub ts_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct PacketLogSnapshot {
    pub packets: Vec<PacketLogItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VersionMismatch {
    pub frontend_version: String,
    pub backend_version: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
pub struct CommandResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_confirmation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_values: Option<JsonMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct FlowDumpItem {
    pub id: u64,
    pub peer_key: String,
    pub client: String,
    pub server: String,
    pub websocket: bool,
    #[serde(rename = "toServer")]
    pub to_server: bool,
    #[serde(rename = "toClient")]
    pub to_client: bool,
    pub is_preferred: bool,
    pub activity: u64,
    pub business_activity: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct FlowDumpResult {
    pub ok: bool,
    pub count: usize,
    pub flows: Vec<FlowDumpItem>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TsumoLoopStatus {
    pub running: bool,
    pub last_reason: String,
    pub win_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AmuletAction {
    BuyPack,
    RefreshShop,
    Skip,
    SelectCandidate,
    SellEffect,
    SortEffect,
    SellRecent,
}

impl AmuletAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BuyPack => "buy_pack",
            Self::RefreshShop => "refresh_shop",
            Self::Skip => "skip",
            Self::SelectCandidate => "select_candidate",
            Self::SellEffect => "sell_effect",
            Self::SortEffect => "sort_effect",
            Self::SellRecent => "sell_recent",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AmuletActionRequest {
    pub action: AmuletAction,
    #[serde(default)]
    pub good_id: Option<u64>,
    #[serde(default)]
    pub selected_index: Option<u64>,
    #[serde(default)]
    pub uid: Option<u64>,
    #[serde(default)]
    pub sorted_uid: Option<Vec<u64>>,
    #[serde(default)]
    pub raw_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AutorunAction {
    Start,
    Stop,
    Step,
    Probe,
    NotifyTestEmail,
    SetMode,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SwitchAction {
    Start,
    StartDebug,
    ListQuads,
    ValidateManualDebug,
    ExecutePlan,
    ExecuteFullPlan,
    Stop,
}

impl SwitchAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::StartDebug => "start_debug",
            Self::ListQuads => "list_quads",
            Self::ValidateManualDebug => "validate_manual_debug",
            Self::ExecutePlan => "execute_plan",
            Self::ExecuteFullPlan => "execute_full_plan",
            Self::Stop => "stop",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRequest {
    pub action: SwitchAction,
    #[serde(default)]
    pub options: Option<JsonMap>,
    #[serde(default)]
    pub snapshot: Option<Value>,
    #[serde(default)]
    pub quad_groups: Option<Value>,
    #[serde(default)]
    pub structure_groups: Option<Value>,
    #[serde(default)]
    pub notify: Option<bool>,
}
