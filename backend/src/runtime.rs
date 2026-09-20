use crate::services::Services;
use crate::{
    pipeline::{
        Direction, ModuleAction, ModuleRegistry, Outcome, Packet, PacketModule, PacketModuleInfo,
        PacketOperation, PacketSubscription, Pipeline, PipelineConfig, GAME_RECORD,
        REPLAY_INJECTOR,
    },
    protocol::{LiqiCodec, MessageType},
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq)]
pub enum FrameOutcome {
    Forward(Vec<u8>),
    Drop,
    Inject {
        frame: Option<Vec<u8>>,
        injected: Vec<InjectedFrame>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct InjectedFrame {
    pub direction: Direction,
    pub frame: Vec<u8>,
}

pub struct FlowProcessor {
    pub(crate) flow_id: u64,
    codec: LiqiCodec,
    pipeline: Pipeline,
    last_business_signal: bool,
    latest_inbound_id: Option<u16>,
    suppressed_response_ids: HashSet<u16>,
    waiters: HashMap<u16, oneshot::Sender<Value>>,
}

impl FlowProcessor {
    pub fn new(config: PipelineConfig) -> Self {
        let registry = Arc::new(ModuleRegistry::default());
        register_builtin_modules(&registry, &config);
        Self::build(config, None, registry)
    }

    pub fn with_services(config: PipelineConfig, services: Arc<Services>) -> Self {
        let registry = Arc::new(ModuleRegistry::default());
        register_builtin_modules(&registry, &config);
        Self::build(config, Some(services), registry)
    }

    pub fn with_registry(
        config: PipelineConfig,
        services: Arc<Services>,
        registry: Arc<ModuleRegistry>,
    ) -> Self {
        Self::build(config, Some(services), registry)
    }

    fn build(
        config: PipelineConfig,
        services: Option<Arc<Services>>,
        registry: Arc<ModuleRegistry>,
    ) -> Self {
        static NEXT_FLOW: AtomicU64 = AtomicU64::new(1);
        let flow_id = NEXT_FLOW.fetch_add(1, Ordering::Relaxed);
        let modules = builtin_modules(services, flow_id);
        Self {
            flow_id,
            codec: LiqiCodec::new(),
            pipeline: Pipeline::with_registry(config, modules, registry),
            last_business_signal: false,
            latest_inbound_id: None,
            suppressed_response_ids: HashSet::new(),
            waiters: HashMap::new(),
        }
    }
}

fn builtin_modules(services: Option<Arc<Services>>, flow_id: u64) -> Vec<Box<dyn PacketModule>> {
    vec![
        Box::new(MethodFilter),
        Box::new(GameRecord(services.clone())),
        Box::new(LimitedTimeActivity),
        Box::new(PacketLogger(services.clone())),
        Box::new(GameState(services.clone(), flow_id)),
        Box::new(UnlockIllustratedBook(services.clone())),
        Box::new(FuseRules(services.clone())),
        Box::new(AutorunEvents(services)),
    ]
}

pub fn register_builtin_modules(registry: &ModuleRegistry, config: &PipelineConfig) {
    let _ = registry.register(PacketModuleInfo::builtin(
        REPLAY_INJECTOR,
        vec![subscription(
            Some(Direction::Outbound),
            Some("Req"),
            None,
            &[PacketOperation::Inject],
        )],
    ));
    for module in builtin_modules(None, 0) {
        let options = config
            .modules
            .iter()
            .find(|entry| entry.id == module.id())
            .map(|entry| &entry.options)
            .unwrap_or(&Value::Null);
        let _ = registry.register(module.registration(options));
    }
}

fn subscription(
    direction: Option<Direction>,
    packet_type: Option<&str>,
    method: Option<&str>,
    operations: &[PacketOperation],
) -> PacketSubscription {
    PacketSubscription {
        direction,
        packet_type: packet_type.map(str::to_owned),
        method: method.map(str::to_owned),
        operations: operations.to_vec(),
    }
}

fn method_subscriptions(
    direction: Direction,
    packet_type: &str,
    methods: &[&str],
    operations: &[PacketOperation],
) -> Vec<PacketSubscription> {
    methods
        .iter()
        .map(|method| subscription(Some(direction), Some(packet_type), Some(method), operations))
        .collect()
}

impl FlowProcessor {
    pub fn set_config(&mut self, config: PipelineConfig) -> Result<(), String> {
        self.pipeline.set_config(config)
    }

    pub fn build_request(
        &mut self,
        id: u16,
        method: &str,
        data: &Value,
    ) -> anyhow::Result<Vec<u8>> {
        self.codec
            .build(MessageType::Request, Some(id), method, data)
    }

    pub fn build_request_auto(
        &mut self,
        start: u16,
        method: &str,
        data: &Value,
    ) -> anyhow::Result<(u16, Vec<u8>)> {
        for offset in 0..=u16::MAX {
            let id = start.wrapping_sub(offset);
            if id != 0 && !self.codec.has_pending(id) {
                return Ok((
                    id,
                    self.codec
                        .build(MessageType::Request, Some(id), method, data)?,
                ));
            }
        }
        anyhow::bail!("no-free-message-id")
    }

    pub fn next_replay_id(&self) -> u16 {
        next_message_id(self.latest_inbound_id.unwrap_or(0))
    }

    pub fn build_replay_request(
        &mut self,
        method: &str,
        data: &Value,
    ) -> anyhow::Result<(u16, Vec<u8>)> {
        let id = self.next_replay_id();
        if self.codec.has_pending(id) {
            anyhow::bail!("replay-message-id-in-use")
        }
        Ok((id, self.build_request(id, method, data)?))
    }

    pub fn cancel_request(&mut self, id: u16) {
        self.codec.cancel_pending(id);
        self.waiters.remove(&id);
        self.suppressed_response_ids.remove(&id);
    }

    pub fn response_waiter(&mut self, id: u16) -> oneshot::Receiver<Value> {
        let (sender, receiver) = oneshot::channel();
        self.waiters.insert(id, sender);
        receiver
    }

    pub fn cancel_waiting_request(&mut self, id: u16) {
        // A completed response may already have freed this ID for client traffic.
        if self.waiters.contains_key(&id) {
            self.cancel_request(id);
        }
    }

    pub fn cancel_requests(&mut self) {
        self.waiters.clear();
        self.suppressed_response_ids.clear();
        self.codec = LiqiCodec::new();
    }

    pub fn last_business_signal(&self) -> bool {
        self.last_business_signal
    }

    pub fn process(&mut self, bytes: &[u8], direction: Direction) -> anyhow::Result<FrameOutcome> {
        self.process_from(bytes, direction, None)
    }

    pub fn process_replay(&mut self, bytes: &[u8]) -> anyhow::Result<FrameOutcome> {
        self.process_from(bytes, Direction::Outbound, Some(REPLAY_INJECTOR))
    }

    pub fn process_injected(&mut self, bytes: &[u8], source: &str) -> anyhow::Result<FrameOutcome> {
        self.process_from(bytes, Direction::Outbound, Some(source))
    }

    fn process_from(
        &mut self,
        bytes: &[u8],
        direction: Direction,
        source: Option<&str>,
    ) -> anyhow::Result<FrameOutcome> {
        self.last_business_signal = false;
        let mut parsed = self.codec.parse(bytes)?;
        if direction == Direction::Inbound {
            if let Some(id) = parsed.id {
                self.latest_inbound_id = Some(id);
            }
        }
        let suppress_response = direction == Direction::Inbound
            && parsed.message_type == MessageType::Response
            && parsed
                .id
                .is_some_and(|id| self.suppressed_response_ids.remove(&id));
        let packet = Packet {
            direction,
            packet_type: match parsed.message_type {
                MessageType::Notify => "Notify",
                MessageType::Request => "Req",
                MessageType::Response => "Res",
            }
            .into(),
            method: parsed.method.to_string(),
            id: parsed.id.map(u32::from),
            data: parsed.data.clone(),
        };
        let response =
            direction == Direction::Inbound && parsed.message_type == MessageType::Response;
        self.last_business_signal = is_business_packet(&packet);
        let pipeline_outcome = match source {
            Some(source) => self
                .pipeline
                .process_after(packet, source)
                .map_err(anyhow::Error::msg)?,
            None => self.pipeline.process(packet),
        };
        let mut response_data = None;
        let outcome = match pipeline_outcome {
            Outcome::Drop => FrameOutcome::Drop,
            Outcome::Forward(packet) => {
                if response {
                    response_data = Some(packet.data.clone());
                }
                parsed.data = packet.data;
                FrameOutcome::Forward(if packet.method != parsed.method.as_ref() {
                    self.codec.build(
                        parsed.message_type,
                        parsed.id,
                        &packet.method,
                        &parsed.data,
                    )?
                } else {
                    self.codec.rebuild(&parsed)?
                })
            }
            Outcome::Inject {
                packet,
                injected,
                forward,
            } => {
                if response && forward {
                    response_data = Some(packet.data.clone());
                }
                parsed.data = packet.data;
                let frame = if forward {
                    Some(if packet.method != parsed.method.as_ref() {
                        self.codec.build(
                            parsed.message_type,
                            parsed.id,
                            &packet.method,
                            &parsed.data,
                        )?
                    } else {
                        self.codec.rebuild(&parsed)?
                    })
                } else {
                    None
                };
                let mut frames = Vec::with_capacity(injected.len());
                for packet in injected {
                    let message_type = message_type(&packet.packet_type)?;
                    if message_type == MessageType::Request {
                        let candidate = packet
                            .id
                            .and_then(|id| u16::try_from(id).ok())
                            .filter(|id| *id != 0)
                            .unwrap_or(u16::MAX);
                        let (id, frame) =
                            self.build_request_auto(candidate, &packet.method, &packet.data)?;
                        self.suppressed_response_ids.insert(id);
                        frames.push(InjectedFrame {
                            direction: packet.direction,
                            frame,
                        });
                    } else {
                        frames.push(InjectedFrame {
                            direction: packet.direction,
                            frame: self.codec.build(
                                message_type,
                                packet.id.and_then(|id| u16::try_from(id).ok()),
                                &packet.method,
                                &packet.data,
                            )?,
                        });
                    }
                }
                FrameOutcome::Inject {
                    frame,
                    injected: frames,
                }
            }
        };
        if response {
            if let Some(sender) = parsed.id.and_then(|id| self.waiters.remove(&id)) {
                if let Some(data) = response_data {
                    let _ = sender.send(data);
                }
            }
        }
        if matches!(
            &outcome,
            FrameOutcome::Drop | FrameOutcome::Inject { frame: None, .. }
        ) && direction == Direction::Outbound
            && parsed.message_type == MessageType::Request
        {
            if let Some(id) = parsed.id {
                self.cancel_request(id);
            }
        }
        if !suppress_response {
            return Ok(outcome);
        }
        match outcome {
            FrameOutcome::Inject { injected, .. } if !injected.is_empty() => {
                Ok(FrameOutcome::Inject {
                    frame: None,
                    injected,
                })
            }
            _ => Ok(FrameOutcome::Drop),
        }
    }
}

fn next_message_id(id: u16) -> u16 {
    let next = id.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

fn is_business_packet(packet: &Packet) -> bool {
    if packet.direction != Direction::Outbound || packet.packet_type != "Req" {
        return false;
    }
    packet.method.starts_with(".lq.Lobby.amuletActivity")
        || matches!(
            packet.method.as_str(),
            ".lq.Lobby.fetchAmuletActivityData" | ".lq.Lobby.loginBeat"
        )
        || (matches!(
            packet.method.as_str(),
            ".lq.Route.requestConnection" | ".lq.Route.requestRouteChange"
        ) && packet.data.get("type").and_then(Value::as_u64) == Some(1))
}

struct PacketLogger(Option<Arc<Services>>);
impl PacketModule for PacketLogger {
    fn id(&self) -> &'static str {
        "packet_logger"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![subscription(None, None, None, &[PacketOperation::Read])]
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            services.record_packet(packet);
        }
        ModuleAction::Forward
    }
}

struct GameRecord(Option<Arc<Services>>);
impl PacketModule for GameRecord {
    fn id(&self) -> &'static str {
        GAME_RECORD
    }

    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![
            subscription(
                Some(Direction::Inbound),
                Some("Res"),
                Some(".lq.Lobby.fetchGameRecord"),
                &[PacketOperation::Read, PacketOperation::Edit],
            ),
            subscription(
                Some(Direction::Outbound),
                Some("Req"),
                Some(".lq.Lobby.fetchGameRecord"),
                &[PacketOperation::Inject],
            ),
        ]
    }

    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            services.apply_game_record_override(packet);
            services.publish_game_record(packet);
        }
        ModuleAction::Forward
    }
}

struct LimitedTimeActivity;
impl PacketModule for LimitedTimeActivity {
    fn id(&self) -> &'static str {
        "limited_time_activity"
    }

    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Outbound,
            "Req",
            &[".lq.Lobby.majClubActivityFinishDay"],
            &[PacketOperation::Read, PacketOperation::Edit],
        )
    }

    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if packet.direction != Direction::Outbound
            || packet.packet_type != "Req"
            || packet.method != ".lq.Lobby.majClubActivityFinishDay"
        {
            return ModuleAction::Forward;
        }
        let Some(customers) = packet
            .data
            .get_mut("customerList")
            .and_then(Value::as_array_mut)
        else {
            return ModuleAction::Forward;
        };
        let mut income = 0_u64;
        for customer in customers.iter_mut().filter_map(Value::as_object_mut) {
            customer.insert("emo".into(), json!(100));
            customer.insert("result".into(), json!(1));
            customer.insert("paymentAmount".into(), json!(500));
            income += 500;
        }
        packet.data["income"] = json!(income);
        ModuleAction::Forward
    }
}

struct GameState(Option<Arc<Services>>, u64);
impl PacketModule for GameState {
    fn id(&self) -> &'static str {
        "game_state"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![
            subscription(
                Some(Direction::Inbound),
                None,
                None,
                &[PacketOperation::Read],
            ),
            subscription(
                Some(Direction::Inbound),
                Some("Res"),
                None,
                &[PacketOperation::Edit],
            ),
        ]
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            services.update_game_state_from_flow(packet, self.1);
            apply_game_response_options(packet, services);
        }
        ModuleAction::Forward
    }
}

fn apply_game_response_options(packet: &mut Packet, services: &Services) {
    if packet.direction != Direction::Inbound || packet.packet_type != "Res" {
        return;
    }
    let game = services.table("game");
    if packet.method == ".lq.Lobby.fetchAnnouncement" && enabled(&game, "modify_announcement", true)
    {
        if let Some(items) = packet
            .data
            .get_mut("announcements")
            .and_then(Value::as_array_mut)
        {
            items.insert(0, services.startup_announcement());
        }
    }
    if packet.method == ".lq.Lobby.fetchAmuletActivityData" {
        let code = services
            .table("general")
            .get("error_code_test")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if code != 0 {
            packet.data =
                json!({"error":{"code":code,"u32Params":[],"strParams":[],"jsonParam":""}});
            return;
        }
    }
    if enabled(&game, "public_all", false) {
        let state = services.game_state();
        let wall = ints(state.get("wall_tiles"));
        let locked = ints(state.get("locked_tiles"));
        let mut pos = (wall.len() + locked.len()) as i64 - 1;
        let mut tiles = Vec::with_capacity(wall.len() + locked.len());
        for id in wall.into_iter().chain(locked) {
            tiles.push(json!({"id":id,"pos":pos}));
            pos -= 1
        }
        replace_desktop_tiles(&mut packet.data, &tiles);
    }
}

struct UnlockIllustratedBook(Option<Arc<Services>>);
impl PacketModule for UnlockIllustratedBook {
    fn id(&self) -> &'static str {
        "unlock_illustrated_book"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Inbound,
            "Res",
            &[".lq.Lobby.amuletActivityFetchBrief"],
            &[PacketOperation::Read, PacketOperation::Edit],
        )
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        let Some(services) = &self.0 else {
            return ModuleAction::Forward;
        };
        if packet.direction == Direction::Inbound
            && packet.packet_type == "Res"
            && packet.method == ".lq.Lobby.amuletActivityFetchBrief"
            && enabled(&services.table("game"), "unlock_illustrated_book", false)
        {
            if let Some(book) = packet.data.get_mut("illustratedBook") {
                book["effectCollection"] = json!((1..500)
                    .flat_map(|id| [id * 10, id * 10 + 1])
                    .collect::<Vec<_>>());
                book["badgeCollection"] = json!((600000..600500).collect::<Vec<_>>());
                book["runeStoneCollection"] = json!((7200..7300).collect::<Vec<_>>());
            }
        }
        ModuleAction::Forward
    }
}

fn replace_desktop_tiles(value: &mut Value, tiles: &[Value]) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "showDesktopTiles" {
                    if child.is_array() {
                        *child = Value::Array(tiles.to_vec())
                    } else if let Some(inner) = child.get_mut("value") {
                        *inner = Value::Array(tiles.to_vec())
                    }
                } else {
                    replace_desktop_tiles(child, tiles)
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_desktop_tiles(item, tiles)
            }
        }
        _ => {}
    }
}

struct FuseRules(Option<Arc<Services>>);
impl PacketModule for FuseRules {
    fn id(&self) -> &'static str {
        "fuse_rules"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Outbound,
            "Req",
            &[
                ".lq.Lobby.amuletActivityGameOperate",
                ".lq.Lobby.amuletActivityOperate",
                ".lq.Lobby.amuletActivityUpgrade",
                ".lq.Lobby.amuletActivityEndShopping",
            ],
            &[PacketOperation::Read, PacketOperation::Drop],
        )
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        let Some(services) = &self.0 else {
            return ModuleAction::Forward;
        };
        if packet.direction != Direction::Outbound || packet.packet_type != "Req" {
            return ModuleAction::Forward;
        }
        let config = services.table("fuse");
        let state = services.game_state();
        let op = packet
            .data
            .get("type")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let effects = state
            .get("effect_list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let candidates = state
            .get("candidate_effect_list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let stage = state.get("stage").and_then(Value::as_i64).unwrap_or(-1);
        let confirm = |title: &str, message: &str, values: Value| {
            if services.confirm(title, message, values) {
                ModuleAction::Forward
            } else {
                ModuleAction::Drop
            }
        };

        if packet.method == ".lq.Lobby.amuletActivityGameOperate"
            && op == 1
            && enabled(&config, "enable_missing_hand_tile_guard", true)
        {
            let mut hand = ints(state.get("hand_tiles"));
            let played = ints(packet.data.get("tileList"));
            let mut missing = Vec::new();
            for tile in &played {
                if let Some(index) = hand.iter().position(|item| item == tile) {
                    hand.remove(index);
                } else {
                    missing.push(*tile);
                }
            }
            if !missing.is_empty() {
                return confirm(
                    "fuse.guard.missingHandTile.title",
                    "fuse.guard.missingHandTile.message",
                    json!({"missingTiles":join(&missing),"playedTiles":join(&played),"handTiles":join(&ints(state.get("hand_tiles")))}),
                );
            }
        }
        if packet.method == ".lq.Lobby.amuletActivityGameOperate"
            && op == 8
            && enabled(&config, "enable_hanabi_win_guard", true)
            && effects
                .iter()
                .any(|item| item.get("id").and_then(Value::as_u64) == Some(2221))
            && state
                .get("ming")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
                < 2
        {
            return confirm(
                "fuse.guard.hanabiWin.title",
                "fuse.guard.hanabiWin.message",
                json!({"hanabiId":2221,"mingCount":state.get("ming").and_then(Value::as_array).map_or(0,Vec::len)}),
            );
        }
        if packet.method == ".lq.Lobby.amuletActivityOperate" {
            if op == 3
                && matches!(stage, 4 | 5)
                && enabled(&config, "enable_ting_ready_skip_guard", true)
                && state
                    .get("ting_list")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
            {
                return confirm(
                    "fuse.guard.tingReadySkip.title",
                    "fuse.guard.tingReadySkip.message",
                    json!({}),
                );
            }
            if op == 3 && matches!(stage, 2 | 9 | 16) && enabled(&config, "enable_skip_guard", true)
            {
                let guard = &config["guard_skip_contains"];
                let watched_a = ints(guard.get("amulets"));
                let watched_b = ints(guard.get("badges"));
                let hits_a = candidates
                    .iter()
                    .filter(|item| watched_a.contains(&base(item)))
                    .count();
                let hits_b = candidates
                    .iter()
                    .filter(|item| watched_b.contains(&badge(item)))
                    .count();
                if hits_a + hits_b > 0 {
                    return confirm(
                        "fuse.guard.skipPack.title",
                        "fuse.guard.skipPack.message",
                        json!({"amuletHitCount":hits_a,"badgeHitCount":hits_b}),
                    );
                }
            }
            if op == 16 && enabled(&config, "enable_shop_force_pick", false) {
                let guard = &config["guard_skip_contains"];
                let watched_a = ints(guard.get("amulets"));
                let watched_b = ints(guard.get("badges"));
                let selected = packet
                    .data
                    .get("args")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(Value::as_u64)
                    .and_then(|i| candidates.get(i as usize));
                let any_hit = candidates.iter().any(|item| {
                    watched_a.contains(&base(item)) || watched_b.contains(&badge(item))
                });
                if any_hit
                    && selected.is_none_or(|item| {
                        !watched_a.contains(&base(item)) && !watched_b.contains(&badge(item))
                    })
                {
                    return confirm(
                        "fuse.guard.forcePick.title",
                        "fuse.guard.forcePick.message",
                        json!({"selBaseId":selected.map(base).unwrap_or(0),"selRawId":selected.and_then(|v|v.get("id")).and_then(Value::as_u64).unwrap_or(0)}),
                    );
                }
            }
            if op == 8 && enabled(&config, "enable_anti_steal_eat", true) {
                let risky = effects.windows(2).any(|pair| {
                    base(&pair[0]) == 230 && badge(&pair[0]) == 600170 && theft_like(&pair[1])
                });
                if risky {
                    return confirm(
                        "fuse.guard.kaviTheft.title",
                        "fuse.guard.kaviTheft.message",
                        json!({"protectedBadges":"600170","pairsText":""}),
                    );
                }
            }
        }
        if packet.method == ".lq.Lobby.amuletActivityUpgrade" {
            if enabled(&config, "enable_prestart_kavi_guard", true) {
                if let Some(index) = effects
                    .iter()
                    .position(|item| base(item) == 230 && badge(item) == 600170)
                {
                    let conduction =
                        effects.iter().filter(|item| badge(item) == 600170).count() as u64;
                    let min = config
                        .get("conduction_min_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(3);
                    if conduction >= min
                        && [index.checked_sub(1), Some(index + 1)]
                            .into_iter()
                            .flatten()
                            .filter_map(|i| effects.get(i))
                            .all(|item| badge(item) != 0)
                    {
                        return confirm(
                            "fuse.guard.kaviPrestartConduction.title",
                            "fuse.guard.kaviPrestartConduction.message",
                            json!({"minCnt":min,"cnt":conduction,"kaviTypeText":if effects[index].get("id").and_then(Value::as_u64).unwrap_or(0)%10==1{"P"}else{"NP"}}),
                        );
                    }
                }
            }
            if enabled(&config, "enable_kavi_plus_buffer_guard", true) {
                if let Some(index) = effects.iter().position(|item| {
                    base(item) == 230
                        && item.get("id").and_then(Value::as_u64).unwrap_or(0) % 10 == 1
                }) {
                    let adjacent = [index.checked_sub(1), Some(index + 1)]
                        .into_iter()
                        .flatten()
                        .filter_map(|i| effects.get(i))
                        .any(|item| badge(item) == 600160);
                    if adjacent {
                        return confirm(
                            "fuse.guard.kaviPrestartExpansion.title",
                            "fuse.guard.kaviPrestartExpansion.message",
                            json!({"expBadgeId":600160}),
                        );
                    }
                }
            }
        }
        if packet.method == ".lq.Lobby.amuletActivityEndShopping" {
            if enabled(&config, "enable_exit_coin_guard", true)
                && effects.iter().any(|item| base(item) == 227)
                && effects.iter().any(|item| base(item) == 207)
                && number(state.get("coin")) != 0
            {
                return confirm(
                    "fuse.guard.exitCoin.title",
                    "fuse.guard.exitCoin.message",
                    json!({"coin":number(state.get("coin")),"moonId":227,"vacuumId":207}),
                );
            }
            if enabled(&config, "enable_exit_life_guard", false)
                && !effects.iter().any(|item| badge(item) == 600100)
            {
                return confirm(
                    "fuse.guard.noLife.title",
                    "fuse.guard.noLife.message",
                    json!({"lifeBadgeId":600100,"amulets":effects}),
                );
            }
        }
        ModuleAction::Forward
    }
}

fn enabled(config: &Value, key: &str, default: bool) -> bool {
    config.get(key).and_then(Value::as_bool).unwrap_or(default)
}
fn ints(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}
fn base(item: &Value) -> u64 {
    item.get("id").and_then(Value::as_u64).unwrap_or(0) / 10
}
fn badge(item: &Value) -> u64 {
    item.get("badge")
        .and_then(|v| v.get("id"))
        .or_else(|| item.get("badgeId"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}
fn theft_like(item: &Value) -> bool {
    match base(item) {
        229 => true,
        228 | 232 => item
            .get("store")
            .and_then(Value::as_array)
            .and_then(|store| store.first())
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .is_some_and(|source| source / 10 == 229),
        _ => false,
    }
}
fn number(value: Option<&Value>) -> u64 {
    value
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0)
}
fn join(values: &[u64]) -> String {
    values
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn message_type(value: &str) -> anyhow::Result<MessageType> {
    match value {
        "Notify" => Ok(MessageType::Notify),
        "Req" => Ok(MessageType::Request),
        "Res" => Ok(MessageType::Response),
        _ => anyhow::bail!("unknown packet type {value}"),
    }
}

struct AutorunEvents(Option<Arc<Services>>);
impl PacketModule for AutorunEvents {
    fn id(&self) -> &'static str {
        "autorun"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Inbound,
            "Res",
            &[
                ".lq.Lobby.amuletActivityStartGame",
                ".lq.Lobby.amuletActivityGiveup",
                ".lq.Lobby.amuletActivityOperate",
                ".lq.Lobby.amuletActivityGameOperate",
                ".lq.Lobby.amuletActivityEndShopping",
            ],
            &[PacketOperation::Read],
        )
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            if packet.direction == Direction::Inbound
                && packet.packet_type == "Res"
                && matches!(
                    packet.method.as_str(),
                    ".lq.Lobby.amuletActivityStartGame"
                        | ".lq.Lobby.amuletActivityGiveup"
                        | ".lq.Lobby.amuletActivityOperate"
                        | ".lq.Lobby.amuletActivityGameOperate"
                        | ".lq.Lobby.amuletActivityEndShopping"
                )
            {
                let _ = services.events.send(crate::services::event(
                    "autorun_packet",
                    json!({"method":packet.method,"id":packet.id,"data":packet.data}),
                ));
            }
        }
        ModuleAction::Forward
    }
}

struct MethodFilter;
impl PacketModule for MethodFilter {
    fn id(&self) -> &'static str {
        "method_filter"
    }
    fn subscriptions(&self, options: &Value) -> Vec<PacketSubscription> {
        options
            .get("bypass_methods")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|method| subscription(None, None, Some(method), &[PacketOperation::Bypass]))
            .collect()
    }
    fn process(&mut self, packet: &mut Packet, options: &Value) -> ModuleAction {
        let bypass = options
            .get("bypass_methods")
            .and_then(Value::as_array)
            .is_some_and(|methods| {
                methods
                    .iter()
                    .any(|method| method.as_str() == Some(&packet.method))
            });
        if bypass {
            ModuleAction::Bypass
        } else {
            ModuleAction::Forward
        }
    }
}

pub fn default_module_options(id: &str) -> Value {
    match id {
        "method_filter" => serde_json::json!({"bypass_methods": [".lq.Route.heartbeat"]}),
        _ => Value::Null,
    }
}

pub fn normalize_config(mut config: PipelineConfig) -> PipelineConfig {
    let defaults = PipelineConfig::default();
    for module in &mut config.modules {
        if module.options.is_null() {
            module.options = default_module_options(&module.id);
        }
    }
    for module in defaults.modules {
        if !config.modules.iter().any(|known| known.id == module.id) {
            if module.id == REPLAY_INJECTOR {
                config.modules.insert(0, module);
            } else {
                config.modules.push(module);
            }
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use std::{fs, path::PathBuf};
    use tokio::sync::broadcast;

    #[derive(Clone, PartialEq, Message)]
    struct Wrapper {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(bytes = "vec", tag = "2")]
        data: Vec<u8>,
    }

    fn request_frame() -> Vec<u8> {
        let wrapper = Wrapper {
            name: ".lq.Lobby.fetchConnectionInfo".into(),
            data: Vec::new(),
        };
        let mut frame = vec![2, 7, 0];
        wrapper.encode(&mut frame).unwrap();
        frame
    }

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "shanten-lens-runtime-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn external_request_replacement_preserves_id_and_receives_server_response() {
        struct Replace;
        impl crate::pipeline::ExternalPacketModule for Replace {
            fn notify(&self, _: &Packet, _: u64) -> Result<(), String> {
                Ok(())
            }
            fn decide(
                &self,
                _: &Packet,
                revision: u64,
                _: std::time::Duration,
            ) -> Result<crate::pipeline::ExternalDecision, String> {
                Ok(crate::pipeline::ExternalDecision::Edit {
                    method: Some(".lq.Lobby.loginBeat".into()),
                    data: json!({"contract":"test"}),
                    expected_revision: revision,
                })
            }
        }
        let registry = Arc::new(ModuleRegistry::default());
        let mut info = PacketModuleInfo::builtin(
            "soulmax",
            vec![subscription(
                Some(Direction::Outbound),
                Some("Req"),
                Some(".lq.Lobby.fetchConnectionInfo"),
                &[PacketOperation::Edit],
            )],
        );
        info.builtin = false;
        info.provider_id = "test".into();
        registry.register_external(info, Arc::new(Replace)).unwrap();
        let mut processor = FlowProcessor::build(
            PipelineConfig {
                schema: 1,
                modules: vec![crate::pipeline::ModuleConfig {
                    id: "soulmax".into(),
                    enabled: true,
                    options: Value::Null,
                }],
            },
            None,
            registry,
        );
        let FrameOutcome::Forward(frame) = processor
            .process(&request_frame(), Direction::Outbound)
            .unwrap()
        else {
            panic!("request not forwarded")
        };
        let mut server = LiqiCodec::new();
        let request = server.parse(&frame).unwrap();
        assert_eq!(request.id, Some(7));
        assert_eq!(request.method.as_ref(), ".lq.Lobby.loginBeat");
        assert_eq!(request.data["contract"], "test");
        let response = server
            .build(
                MessageType::Response,
                Some(7),
                ".lq.Lobby.loginBeat",
                &json!({}),
            )
            .unwrap();
        assert_eq!(
            processor.process(&response, Direction::Inbound).unwrap(),
            FrameOutcome::Forward(response)
        );
        assert!(!processor.codec.has_pending(7));
    }

    #[test]
    fn hanabi_guard_distinguishes_game_operate_from_sort_effect() {
        let root = temp_root("hanabi");
        let (events, mut receiver) = broadcast::channel(32);
        let services = Arc::new(Services::load(root.join("configs"), events).unwrap());
        services.update_game_state(&Packet {direction:Direction::Inbound,packet_type:"Res".into(),method:".lq.Lobby.fetchAmuletActivityData".into(),id:Some(1),data:json!({"game":{"state":{"current":5},"effect":{"effectList":[{"id":2221}]},"round":{"ming":[]}}})});
        let mut guard = FuseRules(Some(services.clone()));
        let mut packet = Packet {
            direction: Direction::Outbound,
            packet_type: "Req".into(),
            method: ".lq.Lobby.amuletActivityOperate".into(),
            id: Some(2),
            data: json!({"type":8,"args":[]}),
        };
        assert!(matches!(
            guard.process(&mut packet, &Value::Null),
            ModuleAction::Forward
        ));
        packet.method = ".lq.Lobby.amuletActivityGameOperate".into();
        let responder = services.clone();
        let answer = std::thread::spawn(move || {
            while let Ok(event) = receiver.blocking_recv() {
                if event["type"] == "msgbox" {
                    assert_eq!(event["data"]["title"], "fuse.guard.hanabiWin.title");
                    responder.resolve_confirmation(event["data"]["id"].as_str().unwrap(), false);
                    return;
                }
            }
        });
        assert!(matches!(
            guard.process(&mut packet, &Value::Null),
            ModuleAction::Drop
        ));
        answer.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn waiters_are_per_flow_and_complete_after_state_commit() {
        let base = temp_root("flow-waiters");
        let (events, _) = broadcast::channel(16);
        let services = Arc::new(Services::load(base.clone(), events).unwrap());
        let config = PipelineConfig {
            schema: 1,
            modules: vec![crate::pipeline::ModuleConfig {
                id: "game_state".into(),
                enabled: true,
                options: Value::Null,
            }],
        };
        let mut a = FlowProcessor::with_services(config.clone(), services.clone());
        let mut b = FlowProcessor::with_services(config, services.clone());
        let method = ".lq.Lobby.amuletActivityOperate";
        for processor in [&mut a, &mut b] {
            processor
                .build_request(42, method, &json!({"type":3}))
                .unwrap();
        }
        let mut waiting_a = a.response_waiter(42);
        let mut waiting_b = b.response_waiter(42);
        let response = LiqiCodec::new()
            .build(
                MessageType::Response,
                Some(42),
                method,
                &json!({"events":[{"type":2,"state":{"current":3}}]}),
            )
            .unwrap();
        b.process(&response, Direction::Inbound).unwrap();
        assert!(waiting_a.try_recv().is_err());
        assert!(waiting_b.try_recv().is_ok());
        assert_eq!(services.game_state()["stage"], 3);
        assert_eq!(services.game_state()["flow_id"], b.flow_id);
        b.build_request(42, method, &json!({"type":3})).unwrap();
        b.cancel_waiting_request(42);
        assert!(
            b.codec.has_pending(42),
            "completed request cleanup must not erase a reused client ID"
        );
        a.cancel_requests();
        assert!(matches!(
            waiting_a.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
        assert!(!a.codec.has_pending(42));
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn unmodified_frame_roundtrips_exactly() {
        let frame = request_frame();
        let output = FlowProcessor::new(PipelineConfig {
            schema: 1,
            modules: vec![],
        })
        .process(&frame, Direction::Outbound)
        .unwrap();
        assert_eq!(output, FrameOutcome::Forward(frame));
    }

    #[test]
    fn real_protobuf_event_response_updates_game_state_end_to_end() {
        let base = temp_root("event");
        let (events, _) = broadcast::channel(4);
        let services = Arc::new(Services::load(base.join("configs"), events).unwrap());
        let config = PipelineConfig {
            schema: 1,
            modules: vec![crate::pipeline::ModuleConfig {
                id: "game_state".into(),
                enabled: true,
                options: Value::Null,
            }],
        };
        let mut processor = FlowProcessor::with_services(config, services.clone());
        let request = processor
            .build_request(
                23,
                ".lq.Lobby.amuletActivityOperate",
                &json!({"activityId":260511,"type":3,"args":[]}),
            )
            .unwrap();
        processor.process(&request, Direction::Outbound).unwrap();
        let response = processor
            .codec
            .build(
                MessageType::Response,
                Some(23),
                ".lq.Lobby.amuletActivityOperate",
                &json!({"events":[{
                    "type":2,
                    "state":{"current":3},
                    "valueChanges":{
                        "character":{
                            "characterId":{"dirty":true,"value":200001},
                            "hp":{"dirty":true,"value":8},
                            "maxHp":{"dirty":true,"value":10}
                        },
                        "map":{"node":{"dirty":true,"value":2}},
                        "round":{"enemy":{"dirty":true,"value":{"hp":"90","maxHp":"120"}}}
                    }
                }]}),
            )
            .unwrap();
        processor.process(&response, Direction::Inbound).unwrap();
        assert_eq!(processor.next_replay_id(), 24);
        assert_eq!(
            processor
                .build_replay_request(
                    ".lq.Lobby.amuletActivityOperate",
                    &json!({"activityId":260511,"type":3,"args":[]}),
                )
                .unwrap()
                .0,
            24
        );

        let state = services.game_state();
        assert_eq!(state["stage"], 3);
        assert_eq!(state["character_id"], 200001);
        assert_eq!(state["hp"], 8);
        assert_eq!(state["max_hp"], 10);
        assert_eq!(state["node"], 2);
        assert_eq!(state["point"], "90");
        assert_eq!(state["target_point"], "120");

        fs::remove_dir_all(base).unwrap();
    }
}
