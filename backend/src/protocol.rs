use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, SerializeOptions};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

const DESCRIPTORS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/liqi_desc.bin"));
static POOL: LazyLock<DescriptorPool> =
    LazyLock::new(|| DescriptorPool::decode(DESCRIPTORS).expect("invalid liqi descriptors"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Notify,
    Request,
    Response,
}

#[derive(Debug, Clone)]
pub struct ParsedMessage {
    pub message_type: MessageType,
    pub id: Option<u16>,
    pub method: Arc<str>,
    pub data: Value,
    descriptor: MessageDescriptor,
}

#[derive(Clone, PartialEq, Message)]
struct Wrapper {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "2")]
    data: Vec<u8>,
}

#[derive(Default)]
pub struct LiqiCodec {
    pending: HashMap<u16, (Arc<str>, MessageDescriptor)>,
}

impl LiqiCodec {
    pub fn new() -> Self {
        LazyLock::force(&POOL);
        Self::default()
    }

    pub fn parse(&mut self, bytes: &[u8]) -> Result<ParsedMessage> {
        ensure!(!bytes.is_empty(), "empty frame");
        match bytes[0] {
            1 => self.parse_notify(bytes),
            2 => self.parse_request(bytes),
            3 => self.parse_response(bytes),
            value => bail!("invalid message type {value}"),
        }
    }

    pub fn rebuild(&self, parsed: &ParsedMessage) -> Result<Vec<u8>> {
        let mut data = parsed.data.clone();
        maybe_encode_action(&mut data)?;
        let dynamic = dynamic_from_json(parsed.descriptor.clone(), &data)?;
        let wrapper = Wrapper {
            name: if parsed.message_type == MessageType::Response {
                String::new()
            } else {
                parsed.method.to_string()
            },
            data: dynamic.encode_to_vec(),
        };
        let mut output = Vec::with_capacity(wrapper.encoded_len() + 3);
        output.push(match parsed.message_type {
            MessageType::Notify => 1,
            MessageType::Request => 2,
            MessageType::Response => 3,
        });
        if let Some(id) = parsed.id {
            output.extend_from_slice(&id.to_le_bytes());
        }
        wrapper.encode(&mut output)?;
        Ok(output)
    }

    pub fn build(
        &mut self,
        message_type: MessageType,
        id: Option<u16>,
        method: &str,
        data: &Value,
    ) -> Result<Vec<u8>> {
        let (descriptor, response) = match message_type {
            MessageType::Notify => (message_descriptor(method)?, None),
            MessageType::Request => {
                let (request, response) = method_descriptors(method)?;
                (request, Some(response))
            }
            MessageType::Response => (method_descriptors(method)?.1, None),
        };
        if let (MessageType::Request, Some(id), Some(response)) = (message_type, id, response) {
            self.pending.insert(id, (Arc::from(method), response));
        }
        self.rebuild(&ParsedMessage {
            message_type,
            id,
            method: Arc::from(method),
            data: data.clone(),
            descriptor,
        })
    }

    pub fn has_pending(&self, id: u16) -> bool {
        self.pending.contains_key(&id)
    }

    pub fn cancel_pending(&mut self, id: u16) {
        self.pending.remove(&id);
    }

    fn parse_notify(&self, bytes: &[u8]) -> Result<ParsedMessage> {
        let wrapper = Wrapper::decode(&bytes[1..])?;
        let descriptor = message_descriptor(&wrapper.name)?;
        parsed(
            MessageType::Notify,
            None,
            wrapper.name,
            wrapper.data,
            descriptor,
        )
    }

    fn parse_request(&mut self, bytes: &[u8]) -> Result<ParsedMessage> {
        ensure!(bytes.len() >= 3, "request frame too short");
        let id = u16::from_le_bytes([bytes[1], bytes[2]]);
        let wrapper = Wrapper::decode(&bytes[3..])?;
        let (request, response) = method_descriptors(&wrapper.name)?;
        let method: Arc<str> = Arc::from(wrapper.name.as_str());
        self.pending.insert(id, (method, response));
        parsed(
            MessageType::Request,
            Some(id),
            wrapper.name,
            wrapper.data,
            request,
        )
    }

    fn parse_response(&mut self, bytes: &[u8]) -> Result<ParsedMessage> {
        ensure!(bytes.len() >= 3, "response frame too short");
        let id = u16::from_le_bytes([bytes[1], bytes[2]]);
        let wrapper = Wrapper::decode(&bytes[3..])?;
        let (method, descriptor) = self
            .pending
            .remove(&id)
            .with_context(|| format!("response {id} has no request"))?;
        parsed(
            MessageType::Response,
            Some(id),
            method.to_string(),
            wrapper.data,
            descriptor,
        )
    }
}

fn parsed(
    kind: MessageType,
    id: Option<u16>,
    method: String,
    bytes: Vec<u8>,
    descriptor: MessageDescriptor,
) -> Result<ParsedMessage> {
    let mut data = dynamic_to_json(&DynamicMessage::decode(
        descriptor.clone(),
        bytes.as_slice(),
    )?)?;
    maybe_decode_action(&mut data)?;
    Ok(ParsedMessage {
        message_type: kind,
        id,
        method: Arc::from(method),
        data,
        descriptor,
    })
}

fn message_descriptor(name: &str) -> Result<MessageDescriptor> {
    POOL.get_message_by_name(name.trim_start_matches('.'))
        .with_context(|| format!("unknown message {name}"))
}

fn method_descriptors(name: &str) -> Result<(MessageDescriptor, MessageDescriptor)> {
    let parts: Vec<_> = name.trim_start_matches('.').split('.').collect();
    ensure!(parts.len() == 3, "invalid rpc method {name}");
    let service = POOL
        .get_service_by_name(&format!("{}.{}", parts[0], parts[1]))
        .with_context(|| format!("unknown service {name}"))?;
    let method = service
        .methods()
        .find(|method| method.name() == parts[2])
        .with_context(|| format!("unknown rpc method {name}"))?;
    Ok((method.input(), method.output()))
}

fn dynamic_to_json(message: &DynamicMessage) -> Result<Value> {
    let options = SerializeOptions::new()
        .stringify_64_bit_integers(false)
        // Keep the packet JSON compatible with Python MessageToDict and the
        // existing frontend/pipeline contract, which use lowerCamelCase.
        .use_proto_field_name(false)
        .skip_default_fields(false);
    let mut serializer = serde_json::Serializer::new(Vec::new());
    message.serialize_with_options(&mut serializer, &options)?;
    Ok(serde_json::from_slice(&serializer.into_inner())?)
}

fn dynamic_from_json(descriptor: MessageDescriptor, value: &Value) -> Result<DynamicMessage> {
    let text = serde_json::to_vec(value)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&text);
    Ok(DynamicMessage::deserialize(descriptor, &mut deserializer)?)
}

fn action_descriptor(name: &str) -> Result<MessageDescriptor> {
    let short = name
        .split('.')
        .rfind(|part| !part.is_empty())
        .context("empty action name")?;
    message_descriptor(&format!("lq.{short}"))
}

fn maybe_decode_action(value: &mut Value) -> Result<()> {
    let Some(name) = value.get("name").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    let Some(encoded) = value.get("data").and_then(Value::as_str) else {
        return Ok(());
    };
    let mut bytes = BASE64.decode(encoded)?;
    xor_action(&mut bytes);
    let action = DynamicMessage::decode(action_descriptor(&name)?, bytes.as_slice())?;
    value["data"] = dynamic_to_json(&action)?;
    Ok(())
}

fn maybe_encode_action(value: &mut Value) -> Result<()> {
    let Some(name) = value.get("name").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    if !value.get("data").is_some_and(Value::is_object) {
        return Ok(());
    }
    let action = dynamic_from_json(action_descriptor(&name)?, &value["data"])?;
    let mut bytes = action.encode_to_vec();
    xor_action(&mut bytes);
    value["data"] = Value::String(BASE64.encode(bytes));
    Ok(())
}

fn xor_action(bytes: &mut [u8]) {
    const KEYS: [u8; 9] = [0x84, 0x5e, 0x4e, 0x42, 0x39, 0xa2, 0x1f, 0x60, 0x1c];
    let base = 23 ^ bytes.len();
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= (base + 5 * index + KEYS[index % KEYS.len()] as usize) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_and_response_correlation() {
        let mut codec = LiqiCodec::new();
        let descriptor = message_descriptor("lq.ReqCommon").unwrap();
        let frame = {
            let wrapper = Wrapper {
                name: ".lq.Lobby.fetchConnectionInfo".into(),
                data: DynamicMessage::new(descriptor).encode_to_vec(),
            };
            let mut bytes = vec![2, 42, 0];
            wrapper.encode(&mut bytes).unwrap();
            bytes
        };
        let parsed = codec.parse(&frame).unwrap();
        assert_eq!(parsed.id, Some(42));
        assert_eq!(codec.rebuild(&parsed).unwrap(), frame);

        let response_descriptor = message_descriptor("lq.ResConnectionInfo").unwrap();
        let wrapper = Wrapper {
            name: String::new(),
            data: DynamicMessage::new(response_descriptor).encode_to_vec(),
        };
        let mut response = vec![3, 42, 0];
        wrapper.encode(&mut response).unwrap();
        let parsed_response = codec.parse(&response).unwrap();
        assert_eq!(&*parsed_response.method, ".lq.Lobby.fetchConnectionInfo");
        assert_eq!(codec.rebuild(&parsed_response).unwrap(), response);
    }

    #[test]
    fn decoded_json_uses_legacy_lower_camel_case_fields() {
        let descriptor = message_descriptor("lq.AmuletValueChanges").unwrap();
        let message = dynamic_from_json(
            descriptor,
            &serde_json::json!({
                "round": {"changeTileCount": {"dirty": true, "value": 2}},
                "map": {"mapNodes": {"dirty": true, "value": []}}
            }),
        )
        .unwrap();
        let value = dynamic_to_json(&message).unwrap();

        assert_eq!(value["round"]["changeTileCount"]["value"], 2);
        assert!(value["map"].get("mapNodes").is_some());
        assert!(value["round"].get("change_tile_count").is_none());
    }
}
