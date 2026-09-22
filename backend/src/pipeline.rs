use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

pub const REPLAY_INJECTOR: &str = "replay_injector";
pub const GAME_RECORD: &str = "game_record";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Outbound,
    Inbound,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Packet {
    pub direction: Direction,
    #[serde(rename = "type")]
    pub packet_type: String,
    pub method: String,
    pub id: Option<u32>,
    pub data: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PacketOperation {
    Read,
    Edit,
    Drop,
    Bypass,
    Inject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PacketSubscription {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub packet_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    pub operations: Vec<PacketOperation>,
}

impl PacketSubscription {
    fn selector_matches(&self, packet: &Packet) -> bool {
        self.direction.is_none_or(|value| value == packet.direction)
            && self
                .packet_type
                .as_deref()
                .is_none_or(|value| value == packet.packet_type)
            && self.method.as_deref().is_none_or(|value| {
                value
                    .strip_suffix('*')
                    .map_or(value == packet.method, |prefix| {
                        packet.method.starts_with(prefix)
                    })
            })
    }

    pub fn matches(&self, packet: &Packet) -> bool {
        self.selector_matches(packet)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PacketModuleInfo {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub description: Option<String>,
    pub builtin: bool,
    pub online: bool,
    pub subscriptions_revision: u64,
    pub subscriptions: Vec<PacketSubscription>,
    pub invocation_count: u64,
    pub total_duration_us: u64,
    pub timeout_count: u64,
    pub dropped_read_count: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExternalDecision {
    Pass,
    Edit {
        #[serde(default)]
        method: Option<String>,
        data: Value,
        expected_revision: u64,
    },
    Drop,
    Bypass,
    Inject {
        packets: Vec<Packet>,
        #[serde(default)]
        drop: bool,
        expected_revision: u64,
    },
}

pub trait ExternalPacketModule: Send + Sync {
    fn notify(&self, packet: &Packet, revision: u64) -> Result<(), String>;
    fn decide(
        &self,
        packet: &Packet,
        revision: u64,
        timeout: Duration,
    ) -> Result<ExternalDecision, String>;
}

impl PacketModuleInfo {
    pub(crate) fn builtin(id: &str, subscriptions: Vec<PacketSubscription>) -> Self {
        Self {
            id: id.into(),
            provider_id: "builtin".into(),
            name: id.into(),
            description: None,
            builtin: true,
            online: true,
            subscriptions_revision: 0,
            subscriptions,
            invocation_count: 0,
            total_duration_us: 0,
            timeout_count: 0,
            dropped_read_count: 0,
        }
    }

    fn matches(&self, packet: &Packet) -> bool {
        self.online
            && self
                .subscriptions
                .iter()
                .any(|subscription| subscription.matches(packet))
    }

    fn allows(&self, packet: &Packet, operation: PacketOperation) -> bool {
        self.online
            && self.subscriptions.iter().any(|subscription| {
                subscription.selector_matches(packet)
                    && subscription.operations.contains(&operation)
            })
    }
}

#[derive(Default)]
pub struct ModuleRegistry {
    modules: RwLock<HashMap<String, RegisteredModule>>,
}

struct RegisteredModule {
    info: PacketModuleInfo,
    external: Option<Arc<dyn ExternalPacketModule>>,
}

impl ModuleRegistry {
    pub fn register(&self, mut module: PacketModuleInfo) -> Result<(), String> {
        validate_module(&module)?;
        let mut modules = self
            .modules
            .write()
            .map_err(|_| "module registry poisoned")?;
        if let Some(current) = modules.get(&module.id) {
            module.subscriptions_revision = if current.info.subscriptions == module.subscriptions {
                current.info.subscriptions_revision
            } else {
                current.info.subscriptions_revision.saturating_add(1)
            };
            copy_stats(&current.info, &mut module);
        }
        modules.insert(
            module.id.clone(),
            RegisteredModule {
                info: module,
                external: None,
            },
        );
        Ok(())
    }

    pub fn register_external(
        &self,
        mut module: PacketModuleInfo,
        handler: Arc<dyn ExternalPacketModule>,
    ) -> Result<(), String> {
        validate_module(&module)?;
        if module.builtin {
            return Err("external module cannot be builtin".into());
        }
        let mut modules = self
            .modules
            .write()
            .map_err(|_| "module registry poisoned")?;
        if let Some(current) = modules.get(&module.id) {
            copy_stats(&current.info, &mut module);
        }
        modules.insert(
            module.id.clone(),
            RegisteredModule {
                info: module,
                external: Some(handler),
            },
        );
        Ok(())
    }

    pub fn replace_subscriptions(
        &self,
        id: &str,
        revision: u64,
        subscriptions: Vec<PacketSubscription>,
    ) -> Result<(), String> {
        validate_subscriptions(&subscriptions)?;
        let mut modules = self
            .modules
            .write()
            .map_err(|_| "module registry poisoned")?;
        let module = &mut modules
            .get_mut(id)
            .ok_or_else(|| format!("unknown module: {id}"))?
            .info;
        if revision <= module.subscriptions_revision {
            return Err(format!(
                "stale subscriptions revision {revision} for {id}; current is {}",
                module.subscriptions_revision
            ));
        }
        module.subscriptions = subscriptions;
        module.subscriptions_revision = revision;
        Ok(())
    }

    pub fn set_online(&self, id: &str, online: bool) -> Result<(), String> {
        let mut modules = self
            .modules
            .write()
            .map_err(|_| "module registry poisoned")?;
        let module = &mut modules
            .get_mut(id)
            .ok_or_else(|| format!("unknown module: {id}"))?
            .info;
        module.online = online;
        Ok(())
    }

    pub fn set_provider_offline(&self, provider_id: &str) {
        let Ok(mut modules) = self.modules.write() else {
            return;
        };
        for module in modules.values_mut() {
            if module.info.provider_id == provider_id {
                module.info.online = false;
                module.external = None;
            }
        }
    }

    pub fn remove_plugin(&self, plugin_id: &str) {
        let prefix = format!("plugin:{plugin_id}:");
        if let Ok(mut modules) = self.modules.write() {
            modules.retain(|id, module| module.info.builtin || !id.starts_with(&prefix));
        }
    }

    pub fn get(&self, id: &str) -> Option<PacketModuleInfo> {
        self.modules
            .read()
            .ok()?
            .get(id)
            .map(|module| module.info.clone())
    }

    pub fn external(&self, id: &str) -> Option<Arc<dyn ExternalPacketModule>> {
        self.modules.read().ok()?.get(id)?.external.clone()
    }

    pub fn snapshot(&self) -> Vec<PacketModuleInfo> {
        let Ok(modules) = self.modules.read() else {
            return Vec::new();
        };
        let mut modules: Vec<_> = modules.values().map(|module| module.info.clone()).collect();
        modules.sort_by(|left, right| left.id.cmp(&right.id));
        modules
    }

    fn record(&self, id: &str, elapsed: Duration, timeout: bool, dropped_read: bool) {
        let Ok(mut modules) = self.modules.write() else {
            return;
        };
        let Some(module) = modules.get_mut(id) else {
            return;
        };
        module.info.invocation_count = module.info.invocation_count.saturating_add(1);
        module.info.total_duration_us = module
            .info
            .total_duration_us
            .saturating_add(elapsed.as_micros().min(u64::MAX as u128) as u64);
        module.info.timeout_count += u64::from(timeout);
        module.info.dropped_read_count += u64::from(dropped_read);
    }
}

fn copy_stats(source: &PacketModuleInfo, target: &mut PacketModuleInfo) {
    target.invocation_count = source.invocation_count;
    target.total_duration_us = source.total_duration_us;
    target.timeout_count = source.timeout_count;
    target.dropped_read_count = source.dropped_read_count;
}

fn validate_module(module: &PacketModuleInfo) -> Result<(), String> {
    if module.id.trim().is_empty() {
        return Err("module id cannot be empty".into());
    }
    if module.provider_id.trim().is_empty() {
        return Err(format!("module {} has no provider", module.id));
    }
    validate_subscriptions(&module.subscriptions)
}

fn validate_subscriptions(subscriptions: &[PacketSubscription]) -> Result<(), String> {
    for subscription in subscriptions {
        if subscription.operations.is_empty() {
            return Err("packet subscription operations cannot be empty".into());
        }
        if subscription.method.as_deref().is_some_and(str::is_empty) {
            return Err("packet subscription method cannot be empty".into());
        }
        if subscription
            .method
            .as_deref()
            .is_some_and(|method| method.strip_suffix('*').unwrap_or(method).contains('*'))
        {
            return Err("packet subscription wildcard is only allowed at the end".into());
        }
        if subscription
            .packet_type
            .as_deref()
            .is_some_and(|value| !matches!(value, "Req" | "Res" | "Notify"))
        {
            return Err("unknown packet subscription type".into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "action", content = "data", rename_all = "snake_case")]
pub enum Outcome {
    Forward(Packet),
    Drop,
    Inject {
        packet: Packet,
        injected: Vec<Packet>,
        forward: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ModuleConfig {
    pub id: String,
    pub enabled: bool,
    #[serde(default)]
    #[specta(type = std::collections::HashMap<String, Value>)]
    pub options: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PipelineConfig {
    pub schema: u32,
    pub modules: Vec<ModuleConfig>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            schema: 1,
            modules: vec![
                module(REPLAY_INJECTOR),
                module("method_filter"),
                module(GAME_RECORD),
                module("packet_logger"),
                module("game_state"),
                module("unlock_illustrated_book"),
                module("fuse_rules"),
                module("autorun"),
            ],
        }
    }
}

fn module(id: &str) -> ModuleConfig {
    ModuleConfig {
        id: id.into(),
        enabled: true,
        options: Value::Null,
    }
}

pub enum ModuleAction {
    Forward,
    Bypass,
    Drop,
    Inject(Vec<Packet>),
}

/// A module receives the same mutable packet in configured order. Mutations are
/// visible to every later module; `Bypass` forwards immediately and `Drop`
/// stops propagation without forwarding.
pub trait PacketModule: Send {
    fn id(&self) -> &'static str;
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![PacketSubscription {
            direction: None,
            packet_type: None,
            method: None,
            operations: vec![PacketOperation::Read],
        }]
    }
    fn registration(&self, options: &Value) -> PacketModuleInfo {
        PacketModuleInfo::builtin(self.id(), self.subscriptions(options))
    }
    fn process(&mut self, packet: &mut Packet, options: &Value) -> ModuleAction;
}

pub struct Pipeline {
    config: PipelineConfig,
    modules: HashMap<String, Box<dyn PacketModule>>,
    registry: Arc<ModuleRegistry>,
}

impl Pipeline {
    pub fn new(
        config: PipelineConfig,
        modules: impl IntoIterator<Item = Box<dyn PacketModule>>,
    ) -> Self {
        Self::with_registry(config, modules, Arc::new(ModuleRegistry::default()))
    }

    pub fn with_registry(
        config: PipelineConfig,
        modules: impl IntoIterator<Item = Box<dyn PacketModule>>,
        registry: Arc<ModuleRegistry>,
    ) -> Self {
        let pipeline = Self {
            config,
            modules: modules
                .into_iter()
                .map(|module| (module.id().to_owned(), module))
                .collect(),
            registry,
        };
        pipeline.refresh_registrations();
        pipeline
    }

    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: PipelineConfig) -> Result<(), String> {
        validate(&config)?;
        self.config = config;
        self.refresh_registrations();
        Ok(())
    }

    pub fn process(&mut self, packet: Packet) -> Outcome {
        self.process_entries(packet, 0, &mut 16)
    }

    pub fn process_after(&mut self, packet: Packet, source: &str) -> Result<Outcome, String> {
        let index = self
            .config
            .modules
            .iter()
            .position(|module| module.id == source)
            .ok_or_else(|| format!("missing pipeline source: {source}"))?;
        if !self.config.modules[index].enabled {
            return Err(format!("pipeline source disabled: {source}"));
        }
        Ok(self.process_entries(packet, index + 1, &mut 16))
    }

    fn process_entries(&mut self, mut packet: Packet, start: usize, budget: &mut usize) -> Outcome {
        let mut injected = Vec::new();
        let mut revision = 0;
        for index in start..self.config.modules.len() {
            let entry = self.config.modules[index].clone();
            if !entry.enabled {
                continue;
            }
            let Some(registration) = self.registry.get(&entry.id) else {
                continue;
            };
            if !registration.matches(&packet) {
                continue;
            }
            if let Some(module) = self.modules.get_mut(&entry.id) {
                let started = Instant::now();
                let original_data = packet.data.clone();
                let action = module.process(&mut packet, &entry.options);
                self.registry
                    .record(&entry.id, started.elapsed(), false, false);
                if packet.data != original_data {
                    if registration.allows(&packet, PacketOperation::Edit) {
                        revision += 1;
                    } else {
                        packet.data = original_data;
                    }
                }
                match action {
                    ModuleAction::Forward => {}
                    ModuleAction::Bypass
                        if registration.allows(&packet, PacketOperation::Bypass) =>
                    {
                        return finish(packet, injected, true)
                    }
                    ModuleAction::Drop if registration.allows(&packet, PacketOperation::Drop) => {
                        return finish(packet, injected, false)
                    }
                    ModuleAction::Inject(packets)
                        if registration.allows(&packet, PacketOperation::Inject) =>
                    {
                        self.process_injections(packets, index + 1, budget, &mut injected);
                    }
                    _ => {}
                }
                continue;
            }

            let Some(module) = self.registry.external(&entry.id) else {
                continue;
            };
            let blocking = [
                PacketOperation::Edit,
                PacketOperation::Drop,
                PacketOperation::Bypass,
                PacketOperation::Inject,
            ]
            .into_iter()
            .any(|operation| registration.allows(&packet, operation));
            if !blocking {
                let started = Instant::now();
                let dropped = module.notify(&packet, revision).is_err();
                self.registry
                    .record(&entry.id, started.elapsed(), false, dropped);
                continue;
            }
            let started = Instant::now();
            let decision = module.decide(&packet, revision, Duration::from_millis(500));
            let timed_out = matches!(&decision, Err(error) if error.contains("timeout"));
            self.registry
                .record(&entry.id, started.elapsed(), timed_out, false);
            match decision {
                Ok(ExternalDecision::Pass) | Err(_) => {}
                Ok(ExternalDecision::Edit {
                    data,
                    method,
                    expected_revision,
                }) if expected_revision == revision
                    && registration.allows(&packet, PacketOperation::Edit) =>
                {
                    if !valid_packet_data(
                        &packet.packet_type,
                        method.as_deref().unwrap_or(&packet.method),
                        &data,
                    ) {
                        continue;
                    }
                    if let Some(method) = method {
                        if packet.direction != Direction::Outbound || packet.packet_type != "Req" {
                            continue;
                        }
                        if packet.method != method {
                            packet.method = method;
                            revision += 1;
                        }
                    }
                    if packet.data != data {
                        packet.data = data;
                        revision += 1;
                    }
                }
                Ok(ExternalDecision::Drop)
                    if registration.allows(&packet, PacketOperation::Drop) =>
                {
                    return finish(packet, injected, false)
                }
                Ok(ExternalDecision::Bypass)
                    if registration.allows(&packet, PacketOperation::Bypass) =>
                {
                    return finish(packet, injected, true)
                }
                Ok(ExternalDecision::Inject {
                    packets,
                    drop,
                    expected_revision,
                }) if expected_revision == revision
                    && packets.len() <= 16
                    && packets.iter().all(valid_injected_packet)
                    && registration.allows(&packet, PacketOperation::Inject)
                    && (!drop || registration.allows(&packet, PacketOperation::Drop)) =>
                {
                    self.process_injections(packets, index + 1, budget, &mut injected);
                    if drop {
                        return finish(packet, injected, false);
                    }
                }
                _ => {}
            }
        }
        finish(packet, injected, true)
    }

    fn process_injections(
        &mut self,
        packets: Vec<Packet>,
        start: usize,
        budget: &mut usize,
        output: &mut Vec<Packet>,
    ) {
        // ponytail: cap the whole injection tree at 16 packets, not 16 per module.
        for packet in packets {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            match self.process_entries(packet, start, budget) {
                Outcome::Forward(packet) => output.push(packet),
                Outcome::Drop => {}
                Outcome::Inject {
                    packet,
                    injected,
                    forward,
                } => {
                    if forward {
                        output.push(packet);
                    }
                    output.extend(injected);
                }
            }
        }
    }

    fn refresh_registrations(&self) {
        for (id, module) in &self.modules {
            let options = self
                .config
                .modules
                .iter()
                .find(|entry| entry.id == *id)
                .map(|entry| &entry.options)
                .unwrap_or(&Value::Null);
            let _ = self.registry.register(module.registration(options));
        }
    }
}

fn finish(packet: Packet, injected: Vec<Packet>, forward: bool) -> Outcome {
    if !injected.is_empty() {
        Outcome::Inject {
            packet,
            injected,
            forward,
        }
    } else if forward {
        Outcome::Forward(packet)
    } else {
        Outcome::Drop
    }
}

fn valid_injected_packet(packet: &Packet) -> bool {
    let valid_shape = match packet.packet_type.as_str() {
        "Req" => packet.direction == Direction::Outbound,
        "Res" => {
            packet.direction == Direction::Inbound
                && packet.id.is_some_and(|id| id > 0 && id <= u16::MAX.into())
        }
        "Notify" => packet.direction == Direction::Inbound && packet.id.is_none(),
        _ => false,
    };
    valid_shape && valid_packet_data(&packet.packet_type, &packet.method, &packet.data)
}

fn valid_packet_data(kind: &str, method: &str, data: &Value) -> bool {
    use crate::protocol::{LiqiCodec, MessageType};
    let kind = match kind {
        "Req" => MessageType::Request,
        "Res" => MessageType::Response,
        "Notify" => MessageType::Notify,
        _ => return false,
    };
    LiqiCodec::new().build(kind, Some(1), method, data).is_ok()
}

pub fn validate(config: &PipelineConfig) -> Result<(), String> {
    if config.schema != 1 {
        return Err(format!("unsupported pipeline schema {}", config.schema));
    }
    let mut seen = std::collections::HashSet::new();
    for module in &config.modules {
        if module.id.trim().is_empty() {
            return Err("module id cannot be empty".into());
        }
        if !seen.insert(&module.id) {
            return Err(format!("duplicate module id: {}", module.id));
        }
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<PipelineConfig, String> {
    if !path.exists() {
        return Ok(PipelineConfig::default());
    }
    let mut config: PipelineConfig =
        serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    validate(&config)?;
    for module in PipelineConfig::default().modules {
        if !config.modules.iter().any(|current| current.id == module.id) {
            if module.id == REPLAY_INJECTOR {
                config.modules.insert(0, module);
            } else {
                config.modules.push(module);
            }
        }
    }
    Ok(config)
}

pub fn save(path: &Path, config: &PipelineConfig) -> Result<(), String> {
    validate(config)?;
    crate::storage::write_json(path, config).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Recorder {
        id: &'static str,
        calls: Arc<Mutex<Vec<&'static str>>>,
        action: ModuleAction,
    }

    impl PacketModule for Recorder {
        fn id(&self) -> &'static str {
            self.id
        }
        fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
            let mut operations = vec![PacketOperation::Read, PacketOperation::Edit];
            match self.action {
                ModuleAction::Drop => operations.push(PacketOperation::Drop),
                ModuleAction::Bypass => operations.push(PacketOperation::Bypass),
                ModuleAction::Inject(_) => operations.push(PacketOperation::Inject),
                ModuleAction::Forward => {}
            }
            vec![PacketSubscription {
                direction: None,
                packet_type: None,
                method: Some("test".into()),
                operations,
            }]
        }
        fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
            self.calls.lock().unwrap().push(self.id);
            packet.data[self.id] = Value::Bool(true);
            std::mem::replace(&mut self.action, ModuleAction::Forward)
        }
    }

    fn packet() -> Packet {
        Packet {
            direction: Direction::Inbound,
            packet_type: "Notify".into(),
            method: "test".into(),
            id: None,
            data: serde_json::json!({}),
        }
    }

    #[test]
    fn module_injection_runs_downstream_once_and_respects_drop() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let modules: Vec<Box<dyn PacketModule>> = vec![
            Box::new(Recorder {
                id: "source",
                calls: calls.clone(),
                action: ModuleAction::Inject(vec![packet()]),
            }),
            Box::new(Recorder {
                id: "edit",
                calls: calls.clone(),
                action: ModuleAction::Forward,
            }),
            Box::new(Recorder {
                id: "drop",
                calls: calls.clone(),
                action: ModuleAction::Drop,
            }),
        ];
        let outcome = Pipeline::new(
            PipelineConfig {
                schema: 1,
                modules: vec![module("source"), module("edit"), module("drop")],
            },
            modules,
        )
        .process(packet());
        // The injection is dropped; the original continues and is forwarded.
        assert!(matches!(outcome, Outcome::Forward(_)));
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["source", "edit", "drop", "edit", "drop"]
        );
    }

    #[test]
    fn configured_order_mutates_then_drop_stops_propagation() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let config = PipelineConfig {
            schema: 1,
            modules: vec![module("second"), module("drop"), module("first")],
        };
        let modules: Vec<Box<dyn PacketModule>> = vec![
            Box::new(Recorder {
                id: "first",
                calls: calls.clone(),
                action: ModuleAction::Forward,
            }),
            Box::new(Recorder {
                id: "second",
                calls: calls.clone(),
                action: ModuleAction::Forward,
            }),
            Box::new(Recorder {
                id: "drop",
                calls: calls.clone(),
                action: ModuleAction::Drop,
            }),
        ];
        assert_eq!(
            Pipeline::new(config, modules).process(packet()),
            Outcome::Drop
        );
        assert_eq!(*calls.lock().unwrap(), vec!["second", "drop"]);
    }

    #[test]
    fn bypass_forwards_packet_and_stops_propagation() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let modules: Vec<Box<dyn PacketModule>> = vec![
            Box::new(Recorder {
                id: "bypass",
                calls: calls.clone(),
                action: ModuleAction::Bypass,
            }),
            Box::new(Recorder {
                id: "after",
                calls: calls.clone(),
                action: ModuleAction::Forward,
            }),
        ];
        let outcome = Pipeline::new(
            PipelineConfig {
                schema: 1,
                modules: vec![module("bypass"), module("after")],
            },
            modules,
        )
        .process(packet());

        assert!(matches!(outcome, Outcome::Forward(_)));
        assert_eq!(*calls.lock().unwrap(), vec!["bypass"]);
    }

    #[test]
    fn injected_packet_starts_after_enabled_source() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let config = PipelineConfig {
            schema: 1,
            modules: vec![module("before"), module(REPLAY_INJECTOR), module("after")],
        };
        let modules: Vec<Box<dyn PacketModule>> = ["before", "after"]
            .into_iter()
            .map(|id| {
                Box::new(Recorder {
                    id,
                    calls: calls.clone(),
                    action: ModuleAction::Forward,
                }) as Box<dyn PacketModule>
            })
            .collect();
        let mut pipeline = Pipeline::new(config, modules);

        assert!(pipeline.process_after(packet(), REPLAY_INJECTOR).is_ok());
        assert_eq!(*calls.lock().unwrap(), vec!["after"]);

        pipeline.config.modules[1].enabled = false;
        assert_eq!(
            pipeline
                .process_after(packet(), REPLAY_INJECTOR)
                .unwrap_err(),
            "pipeline source disabled: replay_injector"
        );
    }
}
