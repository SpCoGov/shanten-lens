use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, EnvFilter, Layer};

pub const LOG_CAPACITY: usize = 1_000;

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct LogEntry {
    pub ts_ms: u64,
    pub level: String,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub fields: HashMap<String, Value>,
}

pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
    events: broadcast::Sender<Value>,
}

impl LogBuffer {
    fn new(events: broadcast::Sender<Value>) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
            events,
        }
    }

    fn push(&self, entry: LogEntry) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() == LOG_CAPACITY {
                entries.pop_front();
            }
            entries.push_back(entry.clone());
        }
        let _ = self
            .events
            .send(json!({"type": "backend_log", "data": entry}));
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default()
    }
}

struct LogLayer {
    buffer: Arc<LogBuffer>,
}

struct FieldVisitor {
    message: Option<String>,
    fields: HashMap<String, Value>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            fields: HashMap::new(),
        }
    }

    fn insert(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            self.message = value
                .as_str()
                .map(str::to_owned)
                .or_else(|| Some(value.to_string()));
        } else {
            self.fields.insert(field.name().to_owned(), value);
        }
    }
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field, Value::String(value.to_owned()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field, Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field, value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field, value.into());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.insert(
            field,
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(value.to_string())),
        );
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.fields
                .insert(field.name().to_owned(), Value::String(value));
        }
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);
        self.buffer.push(LogEntry {
            ts_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or_default(),
            level: metadata.level().to_string(),
            target: metadata.target().to_owned(),
            file: metadata.file().map(str::to_owned),
            line: metadata.line(),
            message: visitor.message.unwrap_or_default(),
            fields: visitor.fields,
        });
    }
}

pub fn init(events: broadcast::Sender<Value>) -> anyhow::Result<Arc<LogBuffer>> {
    let buffer = Arc::new(LogBuffer::new(events));
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "info,shanten_backend::hudsucker=debug,tao::platform_impl::platform::event_loop::runner=error",
        )
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_writer(std::io::stderr),
        )
        .with(LogLayer {
            buffer: buffer.clone(),
        })
        .try_init()?;
    Ok(buffer)
}
