use anyhow::{Result, anyhow};
use serde_json::{Value, json};

/// Protocol types discovered from the Hermes bundle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProtocolType {
    Ug1 = 1,
    Ug2 = 2,
    Ug3 = 3,
}

impl ProtocolType {
    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn definition(&self) -> ProtocolDefinition {
        match self {
            ProtocolType::Ug1 => ProtocolDefinition {
                name: "UG1",
                supports: FeatureSupport {
                    anc: true,
                    game_mode: true,
                    eq: true,
                    dual_link: true,
                    find: true,
                    spatial_audio: true,
                    wear_detection: false,
                    high_quality: true,
                    wind_noise: false,
                    audio_share: false,
                    ai: false,
                },
            },
            ProtocolType::Ug2 => ProtocolDefinition {
                name: "UG2",
                supports: FeatureSupport {
                    anc: true,
                    game_mode: true,
                    eq: true,
                    dual_link: true,
                    find: true,
                    spatial_audio: true,
                    wear_detection: false,
                    high_quality: true,
                    wind_noise: true,
                    audio_share: false,
                    ai: false,
                },
            },
            ProtocolType::Ug3 => ProtocolDefinition {
                name: "UG3",
                supports: FeatureSupport {
                    anc: true,
                    game_mode: true,
                    eq: true,
                    dual_link: true,
                    find: true,
                    spatial_audio: true,
                    wear_detection: true,
                    high_quality: true,
                    wind_noise: true,
                    audio_share: true,
                    ai: true,
                },
            },
        }
    }
}

pub struct ProtocolDefinition {
    pub name: &'static str,
    pub supports: FeatureSupport,
}

#[derive(Clone, serde::Serialize)]
pub struct FeatureSupport {
    pub anc: bool,
    pub game_mode: bool,
    pub eq: bool,
    pub dual_link: bool,
    pub find: bool,
    pub spatial_audio: bool,
    pub wear_detection: bool,
    pub high_quality: bool,
    pub wind_noise: bool,
    pub audio_share: bool,
    pub ai: bool,
}

#[derive(Clone, Debug)]
pub enum ProtocolCommand {
    QueryInfo,
    QueryFirmware,
    SetAnc { mode: String, depth: Option<u8> },
    SetEq { mode: String },
    SetPromptLanguage { value: String },
    SetPromptVolume { value: u8 },
    SetWearDetection { enabled: bool },
    SetGameMode { enabled: bool },
    SetDualConnect { enabled: bool },
    SetSpatialAudio { enabled: bool },
    SetHighQuality { enabled: bool },
    FactoryReset,
}

impl ProtocolCommand {
    pub fn name(&self) -> &'static str {
        match self {
            ProtocolCommand::QueryInfo => "QueryInfo",
            ProtocolCommand::QueryFirmware => "QueryFirmware",
            ProtocolCommand::SetAnc { .. } => "SetAnc",
            ProtocolCommand::SetEq { .. } => "SetEq",
            ProtocolCommand::SetPromptLanguage { .. } => "SetPromptLanguage",
            ProtocolCommand::SetPromptVolume { .. } => "SetPromptVolume",
            ProtocolCommand::SetWearDetection { .. } => "SetWearDetection",
            ProtocolCommand::SetGameMode { .. } => "SetGameMode",
            ProtocolCommand::SetDualConnect { .. } => "SetDualConnect",
            ProtocolCommand::SetSpatialAudio { .. } => "SetSpatialAudio",
            ProtocolCommand::SetHighQuality { .. } => "SetHighQuality",
            ProtocolCommand::FactoryReset => "FactoryReset",
        }
    }

    pub fn payload(&self, protocol: ProtocolType) -> Value {
        match self {
            ProtocolCommand::QueryInfo => json!({
                "list": info_fields(protocol),
                "includeMaxPacketLength": protocol == ProtocolType::Ug1,
            }),
            ProtocolCommand::QueryFirmware => json!({ "list": ["firmwareVersion"] }),
            ProtocolCommand::SetAnc { mode, depth } => {
                if let Some(depth_value) = depth {
                    json!({
                        "type": "ANCDepth",
                        "value": normalize_anc_depth(*depth_value),
                    })
                } else {
                    json!({
                        "type": "ANCType",
                        "value": normalize_anc_mode(mode),
                    })
                }
            }
            ProtocolCommand::SetEq { mode } => json!({
                "mode": normalize_eq_mode(mode),
                "value": eq_mode_to_value(mode),
            }),
            ProtocolCommand::SetPromptLanguage { value } => {
                json!({ "lang": normalize_prompt_language(value) })
            }
            ProtocolCommand::SetPromptVolume { value } => json!({ "volume": value }),
            ProtocolCommand::SetWearDetection { enabled } => json!({ "enabled": enabled }),
            ProtocolCommand::SetGameMode { enabled } => json!({ "enabled": enabled }),
            ProtocolCommand::SetDualConnect { enabled } => json!({ "enabled": enabled }),
            ProtocolCommand::SetSpatialAudio { enabled } => json!({ "enabled": enabled }),
            ProtocolCommand::SetHighQuality { enabled } => json!({ "enabled": enabled }),
            ProtocolCommand::FactoryReset => json!({ "command": "FACTORY_RESET" }),
        }
    }
}

fn info_fields(protocol: ProtocolType) -> Vec<&'static str> {
    match protocol {
        ProtocolType::Ug1 | ProtocolType::Ug2 => vec![
            "maxPacketLength",
            "battery",
            "firmwareVersion",
            "ANCType",
            "ANCDepth",
            "equalizer",
            "gameMode",
            "deviceName",
            "boxMac",
            "windNoise",
            "highQualityDecoding",
            "inBox",
            "connectedDeviceInfo",
            "controlInfo",
            "spatialSoundEffects",
            "deviceDualConnection",
            "promptSoundInfo",
        ],
        ProtocolType::Ug3 => vec![
            "INFO_BATTERY",
            "INFO_VERSION",
            "INFO_BLE_NAME",
            "INFO_EQ",
            "INFO_BUTTONS",
            "INFO_GAME",
            "INFO_PROMPT",
            "INFO_ANC",
            "INFO_ANC_DEPTH",
            "INFO_SPATIAL",
            "INFO_DUAL",
            "INFO_HQ",
            "INFO_BOX_MAC",
            "INFO_WIND_NOISE",
            "INFO_WEAR",
            "INFO_AI",
            "INFO_IN_BOX",
            "INFO_CONNECTED",
            "INFO_CUSTOM_EQ",
            "INFO_TOUCH_SENSITIVITY",
        ],
    }
}

pub struct DecodedFrame {
    pub success: bool,
    pub command: String,
    pub raw: Vec<u8>,
    pub data: Value,
}

pub trait ProtocolFacade {
    fn encode_inner(
        &self,
        protocol: ProtocolType,
        command: &ProtocolCommand,
        payload: &Value,
        seq: u8,
    ) -> Result<Vec<u8>>;
    fn decode_inner(&self, protocol: ProtocolType, bytes: &[u8]) -> Result<DecodedFrame>;
}

pub struct UgProtocolFacade;

impl UgProtocolFacade {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolFacade for UgProtocolFacade {
    fn encode_inner(
        &self,
        protocol: ProtocolType,
        command: &ProtocolCommand,
        payload: &Value,
        seq: u8,
    ) -> Result<Vec<u8>> {
        match protocol {
            ProtocolType::Ug1 => encode_ug1(command, payload),
            ProtocolType::Ug2 => encode_ug2(command, payload, seq),
            other => Err(anyhow!("protocol `{}` is not implemented yet", other.id())),
        }
    }

    fn decode_inner(&self, protocol: ProtocolType, bytes: &[u8]) -> Result<DecodedFrame> {
        match protocol {
            ProtocolType::Ug1 => decode_ug1(bytes),
            ProtocolType::Ug2 => decode_ug2(bytes),
            other => Err(anyhow!("protocol `{}` is not implemented yet", other.id())),
        }
    }
}

const UG1_REQ_HEADER: [u8; 3] = [0xAA, 0xBB, 0xCC];
const UG1_RESP_HEADER: [u8; 3] = [0xDD, 0xEE, 0xFF];
const UG1_HW_HEADER: [u8; 3] = [0x85, 0x86, 0x87];

const UG1_CMD_VERSION: u8 = 1;
const UG1_CMD_DEVICE_STATE: u8 = 4;
const UG1_CMD_EQ: u8 = 5;
const UG1_CMD_DUAL_CONNECT: u8 = 6;
const UG1_CMD_GAME_MODE: u8 = 8;
const UG1_CMD_NOISE_REDUCTION: u8 = 9;
const UG1_CMD_HIGH_QUALITY: u8 = 11;
const UG1_CMD_PROMPT_LANG: u8 = 12;
const UG1_CMD_FACTORY_RESET: u8 = 14;
const UG1_CMD_SOUND_VOLUME: u8 = 17;
const UG1_CMD_SPATIAL_AUDIO: u8 = 18;
const UG1_CMD_WEAR_DETECTION: u8 = 19;

const UG2_CUSTOM_OPCODE: u8 = 0xFF;
const UG2_MAGIC_DEVICE_INFO: u8 = 39;
const UG2_MAGIC_PROMPT: u8 = 41;
const UG2_MAGIC_EQ: u8 = 32;
const UG2_MAGIC_FACTORY_RESET: u8 = 36;
const UG2_MAGIC_ANC: u8 = 53;

const UG2_TAG_BATTERY: u8 = 1;
const UG2_TAG_FIRMWARE_VERSION: u8 = 2;
const UG2_TAG_DEVICE_NAME: u8 = 3;
const UG2_TAG_EQ: u8 = 4;
const UG2_TAG_GAME_MODE: u8 = 8;
const UG2_TAG_PROMPT_SOUND_INFO: u8 = 10;
const UG2_TAG_ANC_TYPE: u8 = 12;
const UG2_TAG_SPATIAL_AUDIO: u8 = 24;
const UG2_TAG_DUAL_LINK: u8 = 25;
const UG2_TAG_CONNECTED_DEVICE_INFO: u8 = 26;
const UG2_TAG_BOX_MAC: u8 = 144;
const UG2_TAG_ANC_DEPTH: u8 = 145;
const UG2_TAG_IN_BOX: u8 = 148;
const UG2_TAG_WIND_NOISE: u8 = 149;
const UG2_TAG_MAX_PACKET_LENGTH: u8 = 255;

pub fn wrap_rcsp_custom(inner: &[u8], seq: u8) -> Vec<u8> {
    let mut body = Vec::with_capacity(inner.len() + 2);
    body.push(seq);
    body.push(UG2_CUSTOM_OPCODE);
    body.extend_from_slice(inner);

    let mut frame = Vec::with_capacity(body.len() + 8);
    frame.extend_from_slice(&[0xFE, 0xDC, 0xBA]);
    frame.push(0xC0);
    frame.push(UG2_CUSTOM_OPCODE);
    frame.push(((body.len() >> 8) & 0xFF) as u8);
    frame.push((body.len() & 0xFF) as u8);
    frame.extend_from_slice(&body);
    frame.push(0xEF);
    frame
}

pub fn unwrap_rcsp_custom(frame: &[u8]) -> Result<Vec<u8>> {
    if frame.len() < 9 {
        return Err(anyhow!("RCSP frame is too short"));
    }
    if frame[0..3] != [0xFE, 0xDC, 0xBA] || *frame.last().unwrap_or(&0) != 0xEF {
        return Err(anyhow!("invalid RCSP framing: {}", hex(frame)));
    }
    if frame[4] != UG2_CUSTOM_OPCODE {
        return Err(anyhow!(
            "unexpected RCSP opcode 0x{:02X}: {}",
            frame[4],
            hex(frame)
        ));
    }
    let param_len = ((frame[5] as usize) << 8) | frame[6] as usize;
    if frame.len() != 8 + param_len {
        return Err(anyhow!("unexpected RCSP packet size: {}", hex(frame)));
    }

    let flags = frame[3];
    let is_command = flags & 0x80 != 0;
    let body = &frame[7..frame.len() - 1];

    if is_command {
        if body.len() < 2 || body[1] != UG2_CUSTOM_OPCODE {
            return Err(anyhow!("invalid RCSP custom command body: {}", hex(frame)));
        }
        return Ok(body[2..].to_vec());
    }

    if body.len() < 3 || body[2] != UG2_CUSTOM_OPCODE {
        return Err(anyhow!("invalid RCSP custom response body: {}", hex(frame)));
    }

    Ok(body[3..].to_vec())
}

pub fn find_complete_rcsp_frame(buffer: &[u8]) -> Option<Vec<u8>> {
    let mut index = 0usize;
    while index + 8 <= buffer.len() {
        if buffer.get(index..index + 3) != Some(&[0xFE, 0xDC, 0xBA]) {
            index += 1;
            continue;
        }
        let param_len = ((buffer[index + 5] as usize) << 8) | buffer[index + 6] as usize;
        let total_len = 8 + param_len;
        if index + total_len > buffer.len() {
            return None;
        }
        if buffer[index + total_len - 1] == 0xEF {
            return Some(buffer[index..index + total_len].to_vec());
        }
        index += 1;
    }
    None
}

pub fn find_complete_ug1_frame(buffer: &[u8]) -> Option<Vec<u8>> {
    let mut index = 0usize;
    while index + 8 <= buffer.len() {
        if buffer.get(index..index + 3) == Some(&UG1_HW_HEADER) {
            if index + 5 <= buffer.len() {
                let command_id = buffer[index + 4];
                let total_len = 5 + ug1_hardware_frame_data_length(command_id);
                if index + total_len <= buffer.len() {
                    return Some(buffer[index..index + total_len].to_vec());
                }
            }
            return None;
        }

        if buffer.get(index..index + 3) != Some(&UG1_RESP_HEADER) {
            index += 1;
            continue;
        }
        let payload_len = buffer[index + 5] as usize;
        let declared_total = 8 + payload_len;
        if index + declared_total <= buffer.len() {
            return Some(buffer[index..index + declared_total].to_vec());
        }

        let max_end = (buffer.len() - index).min(64);
        for total_len in 8..=max_end {
            let frame = &buffer[index..index + total_len];
            let expected_crc =
                u16::from(frame[frame.len() - 1]) << 8 | u16::from(frame[frame.len() - 2]);
            let actual_crc = crc16(&frame[3..frame.len() - 2]);
            if expected_crc == actual_crc {
                return Some(frame.to_vec());
            }
        }
        return None;
    }
    None
}

fn encode_ug1(command: &ProtocolCommand, payload: &Value) -> Result<Vec<u8>> {
    let (command_id, data) = match command {
        ProtocolCommand::QueryInfo => (UG1_CMD_DEVICE_STATE, vec![0]),
        ProtocolCommand::QueryFirmware => (UG1_CMD_VERSION, vec![0]),
        ProtocolCommand::SetAnc { .. } => {
            let anc_type = payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("ANCType");
            let anc_value = payload
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("off");
            let mapped_value = if anc_type == "ANCDepth" {
                map_ug1_anc_depth_value(anc_value)
            } else {
                map_ug1_anc_mode_value(anc_value)
            };
            (UG1_CMD_NOISE_REDUCTION, vec![mapped_value])
        }
        ProtocolCommand::SetEq { .. } => {
            let value = payload
                .get("value")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("UG1 EQ payload is missing numeric value"))?;
            (UG1_CMD_EQ, vec![value as u8])
        }
        ProtocolCommand::SetPromptLanguage { .. } => {
            let lang = payload
                .get("lang")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("UG1 PROMPT_LANG payload is missing numeric lang"))?;
            (UG1_CMD_PROMPT_LANG, vec![lang as u8])
        }
        ProtocolCommand::SetPromptVolume { .. } => {
            let volume = payload
                .get("volume")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("UG1 PROMPT_VOL payload is missing volume"))?;
            (UG1_CMD_SOUND_VOLUME, vec![volume as u8])
        }
        ProtocolCommand::SetWearDetection { .. } => {
            let enabled = payload
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| anyhow!("UG1 WEAR_DETECTION payload is missing enabled"))?;
            (UG1_CMD_WEAR_DETECTION, vec![u8::from(enabled)])
        }
        ProtocolCommand::SetGameMode { .. } => {
            let enabled = payload
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| anyhow!("UG1 GAME_MODE payload is missing enabled"))?;
            (UG1_CMD_GAME_MODE, vec![u8::from(enabled)])
        }
        ProtocolCommand::SetDualConnect { .. } => {
            let enabled = payload
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| anyhow!("UG1 DUAL_CONNECT payload is missing enabled"))?;
            (UG1_CMD_DUAL_CONNECT, vec![u8::from(enabled)])
        }
        ProtocolCommand::SetSpatialAudio { .. } => {
            let enabled = payload
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| anyhow!("UG1 SPATIAL_AUDIO payload is missing enabled"))?;
            (UG1_CMD_SPATIAL_AUDIO, vec![u8::from(enabled)])
        }
        ProtocolCommand::SetHighQuality { .. } => {
            let enabled = payload
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| anyhow!("UG1 HIGH_QUALITY payload is missing enabled"))?;
            (UG1_CMD_HIGH_QUALITY, vec![u8::from(enabled)])
        }
        ProtocolCommand::FactoryReset => (UG1_CMD_FACTORY_RESET, vec![0]),
    };

    let mut packet = Vec::with_capacity(7 + data.len());
    packet.extend_from_slice(&UG1_REQ_HEADER);
    packet.push(command_id);
    packet.push(data.len() as u8);
    packet.extend_from_slice(&data);
    let crc = crc16(&packet[3..]);
    packet.push((crc & 0xFF) as u8);
    packet.push((crc >> 8) as u8);
    Ok(packet)
}

fn decode_ug1(bytes: &[u8]) -> Result<DecodedFrame> {
    if bytes.len() < 8 {
        if bytes.len() >= 5 && bytes[0..3] == UG1_HW_HEADER {
            return decode_ug1_hardware(bytes);
        }
        return Err(anyhow!("UG1 payload is too short"));
    }
    if bytes[0..3] == UG1_HW_HEADER {
        return decode_ug1_hardware(bytes);
    }
    if bytes[0..3] != UG1_RESP_HEADER {
        return Err(anyhow!("invalid UG1 response header: {}", hex(bytes)));
    }

    let command_id = bytes[3];
    let status = bytes[4];
    let payload_len = bytes[5] as usize;
    let expected_len = bytes.len();
    if expected_len < 8 || expected_len < 8 + payload_len {
        return Err(anyhow!("UG1 payload is truncated: {}", hex(bytes)));
    }

    let crc_expected = u16::from(bytes[expected_len - 1]) << 8 | u16::from(bytes[expected_len - 2]);
    let crc_actual = crc16(&bytes[3..expected_len - 2]);
    let payload = &bytes[6..expected_len - 2];
    let command = ug1_command_name(command_id).to_string();
    let mut data = parse_ug1_data(command_id, payload);
    let success = status != 0;
    data["success"] = Value::Bool(success);
    data["crcValid"] = Value::Bool(crc_expected == crc_actual);

    Ok(DecodedFrame {
        success,
        command,
        raw: bytes.to_vec(),
        data,
    })
}

fn decode_ug1_hardware(bytes: &[u8]) -> Result<DecodedFrame> {
    if bytes.len() < 5 || bytes[0..3] != UG1_HW_HEADER {
        return Err(anyhow!("invalid UG1 hardware frame: {}", hex(bytes)));
    }
    let command_id = bytes[4];
    let data_len = ug1_hardware_frame_data_length(command_id);
    if bytes.len() < 5 + data_len {
        return Err(anyhow!("UG1 hardware frame is truncated: {}", hex(bytes)));
    }
    let payload = &bytes[5..5 + data_len];
    Ok(DecodedFrame {
        success: true,
        command: format!("HW_REPORT_{command_id:02X}"),
        raw: bytes[..5 + data_len].to_vec(),
        data: parse_ug1_hardware_data(command_id, payload),
    })
}

fn parse_ug1_data(command_id: u8, payload: &[u8]) -> Value {
    match command_id {
        UG1_CMD_DEVICE_STATE => parse_ug1_device_state(payload),
        UG1_CMD_VERSION => json!({
            "firmwareVersion": bytes_to_ug1_version_string(payload),
        }),
        UG1_CMD_EQ => json!({
            "eqMode": payload.first().copied().map(eq_value_to_mode).unwrap_or("classic"),
        }),
        UG1_CMD_DUAL_CONNECT => json!({
            "dualLink": payload.first().copied().unwrap_or(0) != 0,
        }),
        UG1_CMD_GAME_MODE => json!({
            "gameMode": payload.first().copied().unwrap_or(0) != 0,
        }),
        UG1_CMD_NOISE_REDUCTION => json!({
            "ancMode": payload.first().copied().map(ug1_anc_value_to_mode).unwrap_or("unknown"),
            "ancDepth": payload.first().copied().map(ug1_anc_value_to_depth).unwrap_or("unknown"),
        }),
        UG1_CMD_HIGH_QUALITY => {
            let hq = payload.first().copied().unwrap_or(0) != 0;
            json!({
                "highQuality": hq,
                "gameMode": if hq { false } else { true },
                "dualLink": if hq { false } else { true },
            })
        }
        UG1_CMD_PROMPT_LANG => json!({
            "promptLanguage": payload.first().copied().map(prompt_language_from_value).unwrap_or("Unknown"),
        }),
        UG1_CMD_SOUND_VOLUME => json!({
            "promptVolume": payload.first().copied().unwrap_or(8),
        }),
        UG1_CMD_SPATIAL_AUDIO => json!({
            "spatialAudio": payload.first().copied().unwrap_or(0) != 0,
        }),
        UG1_CMD_WEAR_DETECTION => json!({
            "wearDetection": payload.first().copied().unwrap_or(0) != 0,
        }),
        UG1_CMD_FACTORY_RESET => json!({}),
        _ => json!({
            "success": true,
            "rawPayload": payload.iter().map(|byte| format!("{:02X}", byte)).collect::<String>(),
        }),
    }
}

fn parse_ug1_device_state(payload: &[u8]) -> Value {
    let b0 = payload.first().copied().unwrap_or(0xFF);
    let b1 = payload.get(1).copied().unwrap_or(0xFF);
    let b2 = payload.get(2).copied().unwrap_or(0xFF);
    let anc_raw = payload.get(3).copied().unwrap_or(0);
    let eq_raw = payload.get(4).copied().unwrap_or(0);
    let mut dual_link = payload.get(5).copied().unwrap_or(0) != 0;
    let mut game_mode = payload.get(6).copied().unwrap_or(0) != 0;
    let high_quality = payload.get(7).copied().unwrap_or(0) != 0;
    let sound_type = payload.get(16).copied();
    let prompt_volume = payload.get(19).copied();
    let spatial_audio = payload.get(20).copied().unwrap_or(0) != 0;
    let wear_detection = payload.get(21).copied().unwrap_or(0) != 0;
    let audio_share = payload.get(24).copied().unwrap_or(0) != 0;
    let wind_noise = payload.get(25).copied().unwrap_or(0) != 0;

    if high_quality {
        dual_link = false;
        game_mode = false;
    }

    let mut battery = serde_json::Map::new();
    if b0 <= 100 {
        battery.insert("left".into(), json!(b0));
    }
    if b1 <= 100 {
        battery.insert("right".into(), json!(b1));
    }
    if b2 <= 100 {
        battery.insert("box".into(), json!(b2));
    }
    if battery.is_empty() {
        for &lvl in &[b0, b1, b2] {
            if lvl <= 100 {
                battery.insert("level".into(), json!(lvl));
                break;
            }
        }
    }

    let mut value = json!({
        "battery": battery,
        "ancMode": ug1_anc_value_to_mode(anc_raw),
        "ancDepth": ug1_anc_value_to_depth(anc_raw),
        "eqMode": eq_value_to_mode(eq_raw),
        "dualLink": dual_link,
        "gameMode": game_mode,
        "highQuality": high_quality,
        "spatialAudio": spatial_audio,
        "wearDetection": wear_detection,
        "audioShare": audio_share,
        "windNoise": wind_noise,
    });

    if let Some(kind) = sound_type {
        value["soundType"] = json!(kind);
        value["promptLanguage"] = Value::String(prompt_language_from_value(kind).to_string());
    }
    if let Some(volume) = prompt_volume {
        value["promptVolume"] = json!(volume);
    }

    value
}

fn parse_ug1_hardware_data(command_id: u8, payload: &[u8]) -> Value {
    match command_id {
        1 => {
            let b0 = payload.first().copied().unwrap_or(0xFF);
            let b1 = payload.get(1).copied().unwrap_or(0xFF);
            let b2 = payload.get(2).copied().unwrap_or(0xFF);
            let mut battery = serde_json::Map::new();
            if b0 <= 100 {
                battery.insert("left".into(), json!(b0));
            }
            if b1 <= 100 {
                battery.insert("right".into(), json!(b1));
            }
            if b2 <= 100 {
                battery.insert("box".into(), json!(b2));
            }
            if battery.is_empty() {
                for &lvl in &[b0, b1, b2] {
                    if lvl <= 100 {
                        battery.insert("level".into(), json!(lvl));
                        break;
                    }
                }
            }
            json!({ "battery": battery })
        }
        2 => {
            let noise = payload.first().copied().unwrap_or(0);
            json!({
                "noiseMode": noise,
                "ancMode": ug1_anc_value_to_mode(noise),
                "ancDepth": ug1_anc_value_to_depth(noise),
            })
        }
        4 => json!({ "gameMode": payload.first().copied().unwrap_or(0) != 0 }),
        5 => json!({ "eqMode": payload.first().copied().map(eq_value_to_mode).unwrap_or("classic") }),
        9 => json!({
            "findLeft": payload.first().copied().unwrap_or(0),
            "findRight": payload.get(1).copied().unwrap_or(0),
        }),
        10 => json!({ "spatialAudio": payload.first().copied().unwrap_or(0) != 0 }),
        11 => json!({ "audioShare": payload.first().copied().unwrap_or(0) != 0 }),
        12 => json!({ "dualLink": payload.first().copied().unwrap_or(0) != 0 }),
        14 => json!({ "aiOperation": payload.first().copied().unwrap_or(0) }),
        _ => json!({
            "rawPayload": payload.iter().map(|byte| format!("{:02X}", byte)).collect::<String>(),
        }),
    }
}

fn ug1_hardware_frame_data_length(command_id: u8) -> usize {
    match command_id {
        1 => 3,
        8 => 0,
        9 => 2,
        _ => 1,
    }
}

fn ug1_command_name(command_id: u8) -> &'static str {
    match command_id {
        UG1_CMD_VERSION => "GET_VERSION",
        UG1_CMD_DEVICE_STATE => "GET_INFO",
        UG1_CMD_EQ => "EQ_SET",
        UG1_CMD_DUAL_CONNECT => "DUAL_CONNECT",
        UG1_CMD_GAME_MODE => "GAME_MODE",
        UG1_CMD_NOISE_REDUCTION => "ANC_MODE",
        UG1_CMD_HIGH_QUALITY => "HIGH_QUALITY",
        UG1_CMD_PROMPT_LANG => "PROMPT_LANG",
        UG1_CMD_FACTORY_RESET => "FACTORY_RESET",
        UG1_CMD_SOUND_VOLUME => "PROMPT_VOL",
        UG1_CMD_SPATIAL_AUDIO => "SPATIAL_AUDIO",
        UG1_CMD_WEAR_DETECTION => "WEAR_DETECTION",
        _ => "UNKNOWN",
    }
}

fn encode_ug2(command: &ProtocolCommand, payload: &Value, seq: u8) -> Result<Vec<u8>> {
    let _ = seq;
    let (command_id, data) = match command {
        ProtocolCommand::QueryInfo | ProtocolCommand::QueryFirmware => {
            let list = payload
                .get("list")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("GET_INFO payload is missing the field list"))?;
            let mut data = Vec::with_capacity(list.len() * 2);
            for item in list {
                let field = item
                    .as_str()
                    .ok_or_else(|| anyhow!("GET_INFO field must be a string"))?;
                data.push(ug2_info_type(field));
                data.push(0);
            }
            (UG2_MAGIC_DEVICE_INFO, data)
        }
        ProtocolCommand::SetAnc { .. } => {
            let anc_type = payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("ANCType");
            let anc_value = payload
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or("off");
            let type_marker = if anc_type == "ANCDepth" { 2 } else { 1 };
            let mapped_value = if anc_type == "ANCDepth" {
                map_ug2_anc_depth_value(anc_value)
            } else {
                map_ug2_anc_mode_value(anc_value)
            };
            (UG2_MAGIC_ANC, vec![type_marker, 1, mapped_value])
        }
        ProtocolCommand::SetEq { .. } => {
            let value = payload
                .get("value")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("EQ payload is missing numeric value"))?;
            (UG2_MAGIC_EQ, vec![value as u8])
        }
        ProtocolCommand::SetPromptLanguage { .. } => {
            let lang = payload
                .get("lang")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("PROMPT_LANG payload is missing numeric lang"))?;
            (UG2_MAGIC_PROMPT, vec![1, 1, lang as u8])
        }
        ProtocolCommand::SetPromptVolume { .. } => {
            let volume = payload
                .get("volume")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("PROMPT_VOL payload is missing volume"))?;
            (UG2_MAGIC_PROMPT, vec![2, 1, volume as u8])
        }
        ProtocolCommand::SetWearDetection { .. } => {
            return Err(anyhow!("UG2 wear detection is not implemented yet"));
        }
        ProtocolCommand::SetGameMode { .. }
        | ProtocolCommand::SetDualConnect { .. }
        | ProtocolCommand::SetSpatialAudio { .. }
        | ProtocolCommand::SetHighQuality { .. } => {
            return Err(anyhow!("UG2 command `{}` is not implemented yet", command.name()));
        }
        ProtocolCommand::FactoryReset => (UG2_MAGIC_FACTORY_RESET, Vec::new()),
    };

    let mut packet = vec![command_id, 1, 0, data.len() as u8];
    packet.extend_from_slice(&data);
    Ok(packet)
}

fn decode_ug2(bytes: &[u8]) -> Result<DecodedFrame> {
    if bytes.len() < 4 {
        return Err(anyhow!("UG2 payload is too short"));
    }

    let command_id = bytes[0];
    let payload_len = bytes[3] as usize;
    let available = bytes.len().saturating_sub(4);
    let payload = &bytes[4..4 + payload_len.min(available)];
    let command = ug2_command_name(command_id).to_string();
    let data = parse_ug2_data(command_id, payload);
    let success = data
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(command == "GET_INFO");

    Ok(DecodedFrame {
        success,
        command,
        raw: bytes.to_vec(),
        data,
    })
}

fn parse_ug2_data(command_id: u8, payload: &[u8]) -> Value {
    match command_id {
        UG2_MAGIC_DEVICE_INFO => parse_ug2_device_info(payload),
        UG2_MAGIC_ANC => {
            let success = payload.last().copied().unwrap_or(1) == 0;
            let mut value = json!({ "success": success });
            if payload.len() >= 2 {
                let anc_type = if payload[0] == 1 {
                    "ANCType"
                } else {
                    "ANCDepth"
                };
                value["type"] = Value::String(anc_type.to_string());
                value["value"] = Value::String(if payload[0] == 1 {
                    anc_mode_from_value(payload[1]).to_string()
                } else {
                    anc_depth_from_value(payload[1]).to_string()
                });
            }
            value
        }
        UG2_MAGIC_EQ => json!({
            "success": payload.last().copied().unwrap_or(1) == 0,
            "eqMode": payload.first().copied().map(eq_value_to_mode).unwrap_or("classic"),
        }),
        UG2_MAGIC_PROMPT => json!({
            "success": payload.last().copied().unwrap_or(1) == 0,
        }),
        UG2_MAGIC_FACTORY_RESET => json!({
            "success": payload.last().copied().unwrap_or(1) == 0,
        }),
        _ => json!({
            "success": payload.last().copied().unwrap_or(1) == 0,
            "rawPayload": payload.iter().map(|byte| format!("{:02X}", byte)).collect::<String>(),
        }),
    }
}

fn parse_ug2_device_info(payload: &[u8]) -> Value {
    let mut value = json!({});
    let mut index = 0usize;
    while index + 1 < payload.len() {
        let tag = payload[index];
        let len = payload[index + 1] as usize;
        index += 2;
        if index + len > payload.len() {
            break;
        }
        let item = &payload[index..index + len];
        match tag {
            UG2_TAG_BATTERY if len >= 2 => {
                value["battery"] = json!({
                    "left": item[0],
                    "right": item[1],
                    "box": item.get(2).copied(),
                });
            }
            UG2_TAG_FIRMWARE_VERSION if !item.is_empty() => {
                value["firmwareVersion"] = Value::String(bytes_to_version_string(item));
            }
            UG2_TAG_DEVICE_NAME if !item.is_empty() => {
                value["deviceName"] = Value::String(bytes_to_string(item));
            }
            UG2_TAG_EQ if !item.is_empty() => {
                value["eqMode"] = Value::String(eq_value_to_mode(item[0]).to_string());
            }
            UG2_TAG_GAME_MODE if !item.is_empty() => {
                value["gameMode"] = Value::Bool(item[0] != 0);
            }
            UG2_TAG_PROMPT_SOUND_INFO if len >= 2 => {
                value["promptLanguage"] = Value::String(prompt_language_from_value(item[0]).into());
                value["promptVolume"] = json!(item[1]);
            }
            UG2_TAG_ANC_TYPE if !item.is_empty() => {
                value["ancMode"] = Value::String(anc_mode_from_value(item[0]).to_string());
            }
            UG2_TAG_ANC_DEPTH if !item.is_empty() => {
                value["ancDepth"] = Value::String(anc_depth_from_value(item[0]).to_string());
            }
            UG2_TAG_SPATIAL_AUDIO if !item.is_empty() => {
                value["spatialAudio"] = Value::Bool(item[0] != 0);
            }
            UG2_TAG_DUAL_LINK if !item.is_empty() => {
                value["dualLink"] = Value::Bool(item[0] != 0);
            }
            UG2_TAG_CONNECTED_DEVICE_INFO if len >= 7 => {
                value["connectedDevice"] = json!({
                    "mobileMac": bytes_to_mac(&item[..6]),
                    "mobilePhoneName": bytes_to_string(&item[6..]),
                });
            }
            UG2_TAG_BOX_MAC if len >= 6 => {
                value["boxMac"] = Value::String(bytes_to_mac(item));
            }
            UG2_TAG_IN_BOX if !item.is_empty() => {
                value["inBox"] = Value::Bool(item[0] != 0);
            }
            UG2_TAG_WIND_NOISE if !item.is_empty() => {
                value["windNoise"] = Value::Bool(item[0] != 0);
            }
            UG2_TAG_MAX_PACKET_LENGTH if !item.is_empty() => {
                value["maxPacketLength"] = json!(item[0]);
            }
            _ => {}
        }
        index += len;
    }
    value["success"] = Value::Bool(true);
    value
}

fn ug2_command_name(command_id: u8) -> &'static str {
    match command_id {
        UG2_MAGIC_DEVICE_INFO => "GET_INFO",
        UG2_MAGIC_EQ => "EQ_SET",
        UG2_MAGIC_FACTORY_RESET => "FACTORY_RESET",
        UG2_MAGIC_PROMPT => "PROMPT",
        UG2_MAGIC_ANC => "ANC_MODE",
        _ => "UNKNOWN",
    }
}

fn ug2_info_type(field: &str) -> u8 {
    match field {
        "battery" => UG2_TAG_BATTERY,
        "firmwareVersion" => UG2_TAG_FIRMWARE_VERSION,
        "deviceName" | "bleName" => UG2_TAG_DEVICE_NAME,
        "equalizer" => UG2_TAG_EQ,
        "gameMode" => UG2_TAG_GAME_MODE,
        "promptSoundInfo" => UG2_TAG_PROMPT_SOUND_INFO,
        "ANCType" => UG2_TAG_ANC_TYPE,
        "ANCDepth" => UG2_TAG_ANC_DEPTH,
        "spatialSoundEffects" => UG2_TAG_SPATIAL_AUDIO,
        "deviceDualConnection" => UG2_TAG_DUAL_LINK,
        "connectedDeviceInfo" => UG2_TAG_CONNECTED_DEVICE_INFO,
        "boxMac" => UG2_TAG_BOX_MAC,
        "windNoise" => UG2_TAG_WIND_NOISE,
        "inBox" => UG2_TAG_IN_BOX,
        "maxPacketLength" => UG2_TAG_MAX_PACKET_LENGTH,
        other => {
            let _ = other;
            UG2_TAG_BATTERY
        }
    }
}

fn normalize_anc_mode(mode: &str) -> &'static str {
    match mode.to_ascii_lowercase().as_str() {
        "off" => "off",
        "transparent" | "transparency" => "transparent",
        "light" => "light",
        "medium" => "medium",
        "deep" => "deep",
        "auto" | "adaptive" | "anc" | "on" => "adaptive",
        _ => "adaptive",
    }
}

fn normalize_anc_depth(depth: u8) -> &'static str {
    match depth {
        0 | 1 => "light",
        2 | 3 => "medium",
        _ => "deep",
    }
}

fn map_ug1_anc_mode_value(value: &str) -> u8 {
    match value {
        "off" => 208, // 0xD0
        "transparent" | "transparency" => 210, // 0xD2
        "deep" => 161, // 0xA1
        "medium" => 177, // 0xB1
        "light" => 193, // 0xC1
        "adaptive" => 209, // 0xD1
        _ => 209,
    }
}

fn map_ug1_anc_depth_value(value: &str) -> u8 {
    match value {
        "deep" => 161, // 0xA1
        "medium" => 177, // 0xB1
        "light" => 193, // 0xC1
        "adaptive" => 209, // 0xD1
        "transparent" | "transparency" => 210, // 0xD2
        "off" => 208, // 0xD0
        _ => 177,
    }
}

fn map_ug2_anc_mode_value(value: &str) -> u8 {
    match value {
        "off" => 0,
        "adaptive" => 1,
        "transparent" => 2,
        _ => 0,
    }
}

fn map_ug2_anc_depth_value(value: &str) -> u8 {
    match value {
        "deep" => 0,
        "medium" => 1,
        "light" => 2,
        _ => 0,
    }
}

fn ug1_anc_value_to_mode(value: u8) -> &'static str {
    match value {
        0 | 160 | 176 | 192 | 208 => "off",
        2 | 162 | 178 | 194 | 210 => "transparent",
        1 | 161 | 177 | 193 | 209 => "adaptive",
        _ => "unknown",
    }
}

fn ug1_anc_value_to_depth(value: u8) -> &'static str {
    match value {
        160..=162 => "deep",
        176..=178 => "medium",
        192..=194 => "light",
        208..=210 | 0..=2 => "adaptive",
        _ => "unknown",
    }
}

fn anc_mode_from_value(value: u8) -> &'static str {
    match value {
        0 => "off",
        1 => "adaptive",
        2 => "transparent",
        _ => "unknown",
    }
}

fn anc_depth_from_value(value: u8) -> &'static str {
    match value {
        0 => "deep",
        1 => "medium",
        2 => "light",
        _ => "unknown",
    }
}

fn normalize_eq_mode(mode: &str) -> &'static str {
    match mode.to_ascii_lowercase().as_str() {
        "jazz" => "jazz",
        "electronic" => "electronic",
        "pop" | "popular" => "pop",
        "classical" => "classical",
        "rock" => "rock",
        "bass" => "bass",
        "treble" => "treble",
        "classic" | "balanced" | "default" => "classic",
        _ => "classic",
    }
}

fn eq_mode_to_value(mode: &str) -> u8 {
    match normalize_eq_mode(mode) {
        "jazz" => 1,
        "electronic" => 2,
        "pop" => 3,
        "classical" => 4,
        "rock" => 5,
        "bass" => 6,
        "treble" => 7,
        "classic" => 8,
        _ => 8,
    }
}

fn eq_value_to_mode(value: u8) -> &'static str {
    match value {
        1 => "jazz",
        2 => "electronic",
        3 => "pop",
        4 => "classical",
        5 => "rock",
        6 => "bass",
        7 => "treble",
        8 => "classic",
        _ => "classic",
    }
}

fn normalize_prompt_language(value: &str) -> u8 {
    match value.to_ascii_lowercase().as_str() {
        "english" | "en" | "en-us" => 0,
        "chinese" | "zh" | "zh-cn" => 1,
        _ => 0,
    }
}

fn prompt_language_from_value(value: u8) -> &'static str {
    match value {
        0 => "English",
        1 => "Chinese",
        _ => "Unknown",
    }
}

fn bytes_to_string(bytes: &[u8]) -> String {
    let bytes = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn bytes_to_version_string(bytes: &[u8]) -> String {
    if bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b'.')
    {
        return bytes_to_string(bytes);
    }
    bytes
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

fn bytes_to_ug1_version_string(bytes: &[u8]) -> String {
    let primary = bytes.get(0..3).unwrap_or(&[]);
    if primary.iter().any(|byte| *byte != 0) && primary.len() == 3 {
        return format!("{}.{}.{}", primary[0], primary[1], primary[2]);
    }

    let secondary = bytes.get(3..6).unwrap_or(&[]);
    if secondary.iter().any(|byte| *byte != 0) && secondary.len() == 3 {
        return format!("{}.{}.{}", secondary[0], secondary[1], secondary[2]);
    }

    "0.0.0".to_string()
}

fn bytes_to_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<Vec<_>>()
        .join(":")
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<String>()
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for byte in bytes {
        let mut value = crc ^ u16::from(*byte);
        for _ in 0..8 {
            value = if value & 1 != 0 {
                (value >> 1) ^ 0xA001
            } else {
                value >> 1
            };
        }
        crc = value;
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ug1_encode_switches() {
        let commands = [
            (ProtocolCommand::SetGameMode { enabled: true }, UG1_CMD_GAME_MODE, vec![1]),
            (ProtocolCommand::SetGameMode { enabled: false }, UG1_CMD_GAME_MODE, vec![0]),
            (ProtocolCommand::SetDualConnect { enabled: true }, UG1_CMD_DUAL_CONNECT, vec![1]),
            (ProtocolCommand::SetDualConnect { enabled: false }, UG1_CMD_DUAL_CONNECT, vec![0]),
            (ProtocolCommand::SetSpatialAudio { enabled: true }, UG1_CMD_SPATIAL_AUDIO, vec![1]),
            (ProtocolCommand::SetSpatialAudio { enabled: false }, UG1_CMD_SPATIAL_AUDIO, vec![0]),
            (ProtocolCommand::SetHighQuality { enabled: true }, UG1_CMD_HIGH_QUALITY, vec![1]),
            (ProtocolCommand::SetHighQuality { enabled: false }, UG1_CMD_HIGH_QUALITY, vec![0]),
        ];

        for (cmd, expected_cmd_id, expected_data) in commands {
            let payload = cmd.payload(ProtocolType::Ug1);
            let encoded = encode_ug1(&cmd, &payload).expect("encode_ug1 failed");
            assert_eq!(encoded[0..3], UG1_REQ_HEADER);
            assert_eq!(encoded[3], expected_cmd_id);
            assert_eq!(encoded[4], expected_data.len() as u8);
            assert_eq!(&encoded[5..5 + expected_data.len()], expected_data.as_slice());
        }
    }

    #[test]
    fn test_ug1_anc_mappings() {
        assert_eq!(map_ug1_anc_mode_value("off"), 208);
        assert_eq!(map_ug1_anc_mode_value("transparent"), 210);
        assert_eq!(map_ug1_anc_mode_value("transparency"), 210);
        assert_eq!(map_ug1_anc_mode_value("adaptive"), 209);
        assert_eq!(map_ug1_anc_mode_value("deep"), 161);
        assert_eq!(map_ug1_anc_mode_value("medium"), 177);
        assert_eq!(map_ug1_anc_mode_value("light"), 193);

        assert_eq!(ug1_anc_value_to_mode(208), "off");
        assert_eq!(ug1_anc_value_to_mode(210), "transparent");
        assert_eq!(ug1_anc_value_to_mode(209), "adaptive");
        assert_eq!(ug1_anc_value_to_mode(161), "adaptive");
        assert_eq!(ug1_anc_value_to_mode(177), "adaptive");
        assert_eq!(ug1_anc_value_to_mode(193), "adaptive");

        assert_eq!(ug1_anc_value_to_depth(161), "deep");
        assert_eq!(ug1_anc_value_to_depth(177), "medium");
        assert_eq!(ug1_anc_value_to_depth(193), "light");
        assert_eq!(ug1_anc_value_to_depth(209), "adaptive");
        assert_eq!(ug1_anc_value_to_depth(210), "adaptive");
    }

    #[test]
    fn test_eq_mappings() {
        let expected = [
            ("jazz", 1),
            ("electronic", 2),
            ("pop", 3),
            ("classical", 4),
            ("rock", 5),
            ("bass", 6),
            ("treble", 7),
            ("classic", 8),
        ];

        for (name, val) in expected {
            assert_eq!(eq_mode_to_value(name), val);
            assert_eq!(eq_value_to_mode(val), name);
        }
    }

    #[test]
    fn test_prompt_language() {
        assert_eq!(normalize_prompt_language("english"), 0);
        assert_eq!(normalize_prompt_language("en"), 0);
        assert_eq!(normalize_prompt_language("chinese"), 1);
        assert_eq!(normalize_prompt_language("zh"), 1);

        assert_eq!(prompt_language_from_value(0), "English");
        assert_eq!(prompt_language_from_value(1), "Chinese");
    }

    #[test]
    fn test_ug1_device_state_battery_and_ldac_exclusion() {
        // [b0, b1, b2, anc, eq, dual_link, game_mode, high_quality, ...]
        let mut payload = vec![0u8; 30];
        payload[0] = 0xFF; // single headphone left invalid
        payload[1] = 85;   // right = 85%
        payload[2] = 0xFF; // box invalid
        payload[5] = 1;    // dual_link requested true
        payload[6] = 1;    // game_mode requested true
        payload[7] = 1;    // high_quality (LDAC) is ON

        let state = parse_ug1_device_state(&payload);
        assert_eq!(state["battery"]["right"], 85);
        assert!(state["battery"]["left"].is_null());
        assert_eq!(state["highQuality"], true);
        // LDAC mutual exclusion
        assert_eq!(state["dualLink"], false);
        assert_eq!(state["gameMode"], false);
    }

    #[test]
    fn test_ug1_hardware_report_parsing() {
        let game_mode_data = parse_ug1_hardware_data(4, &[1]);
        assert_eq!(game_mode_data["gameMode"], true);

        let eq_data = parse_ug1_hardware_data(5, &[1]);
        assert_eq!(eq_data["eqMode"], "jazz");

        let spatial_data = parse_ug1_hardware_data(10, &[1]);
        assert_eq!(spatial_data["spatialAudio"], true);

        let dual_link_data = parse_ug1_hardware_data(12, &[1]);
        assert_eq!(dual_link_data["dualLink"], true);
    }
}

