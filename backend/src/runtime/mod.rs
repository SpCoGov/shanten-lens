//! WebSocket frame processing and built-in packet modules.
mod builtin;
mod fuse;
#[cfg(test)]
mod tests;

use crate::services::Services;
use crate::{
    pipeline::{
        Direction, ModuleRegistry, Outcome, Packet, Pipeline, PipelineConfig, REPLAY_INJECTOR,
    },
    protocol::{LiqiCodec, MessageType},
};
use builtin::builtin_modules;
pub use builtin::{default_module_options, normalize_config, register_builtin_modules};
use serde_json::Value;
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
    services: Option<Arc<Services>>,
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
        let modules = builtin_modules(services.clone(), flow_id);
        Self {
            flow_id,
            codec: LiqiCodec::new(),
            pipeline: Pipeline::with_registry(config, modules, registry),
            services,
            last_business_signal: false,
            latest_inbound_id: None,
            suppressed_response_ids: HashSet::new(),
            waiters: HashMap::new(),
        }
    }
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
        if let Some(services) = &self.services {
            services.record_received_packet(&packet);
        }
        let response =
            direction == Direction::Inbound && parsed.message_type == MessageType::Response;
        self.last_business_signal = is_business_packet(&packet);
        // Keep transport, recordings and the inactive source's state cache alive, but
        // bypass packet edits, plugins, guards and UI events for an unselected source.
        let bypass = self.services.as_ref().is_some_and(|services| !services.packet_processing_allowed());
        let pipeline_outcome = if bypass {
            let services = self.services.as_ref().unwrap();
            services.update_game_state_from_flow(&packet, self.flow_id);
            services.record_packet(&packet);
            Outcome::Forward(packet)
        } else { match source {
            Some(source) => self
                .pipeline
                .process_after(packet, source)
                .map_err(anyhow::Error::msg)?,
            None => self.pipeline.process(packet),
        }};
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

fn message_type(value: &str) -> anyhow::Result<MessageType> {
    match value {
        "Notify" => Ok(MessageType::Notify),
        "Req" => Ok(MessageType::Request),
        "Res" => Ok(MessageType::Response),
        _ => anyhow::bail!("unknown packet type {value}"),
    }
}
