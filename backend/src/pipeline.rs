use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::{collections::HashMap, fs, path::Path};

pub const REPLAY_INJECTOR: &str = "replay_injector";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "action", content = "data", rename_all = "snake_case")]
pub enum Outcome {
    Forward(Packet),
    Drop,
    Inject {
        packet: Packet,
        injected: Vec<Packet>,
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
                module("packet_logger"),
                module("limited_time_activity"),
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
    fn process(&mut self, packet: &mut Packet, options: &Value) -> ModuleAction;
}

pub struct Pipeline {
    config: PipelineConfig,
    modules: HashMap<String, Box<dyn PacketModule>>,
}

impl Pipeline {
    pub fn new(
        config: PipelineConfig,
        modules: impl IntoIterator<Item = Box<dyn PacketModule>>,
    ) -> Self {
        Self {
            config,
            modules: modules
                .into_iter()
                .map(|module| (module.id().to_owned(), module))
                .collect(),
        }
    }

    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: PipelineConfig) -> Result<(), String> {
        validate(&config)?;
        self.config = config;
        Ok(())
    }

    pub fn process(&mut self, packet: Packet) -> Outcome {
        self.process_entries(packet, 0)
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
        Ok(self.process_entries(packet, index + 1))
    }

    fn process_entries(&mut self, mut packet: Packet, start: usize) -> Outcome {
        let mut injected = Vec::new();
        for entry in self.config.modules.iter().skip(start) {
            if !entry.enabled {
                continue;
            }
            let Some(module) = self.modules.get_mut(&entry.id) else {
                continue;
            };
            match module.process(&mut packet, &entry.options) {
                ModuleAction::Forward => {}
                ModuleAction::Bypass => return Outcome::Forward(packet),
                ModuleAction::Drop => return Outcome::Drop,
                ModuleAction::Inject(mut packets) => injected.append(&mut packets),
            }
        }
        if injected.is_empty() {
            Outcome::Forward(packet)
        } else {
            Outcome::Inject { packet, injected }
        }
    }
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
    let parent = path
        .parent()
        .ok_or("pipeline config has no parent directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
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
