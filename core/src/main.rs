mod protocol;
mod transport;

use anyhow::{Result, anyhow};
use chrono::Utc;
use clap::{Parser, Subcommand};
use protocol::{FeatureSupport, ProtocolCommand, ProtocolType, UG1_REJECTED};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::Duration;
use transport::{LinuxTransport, Transport, TransportTarget};

const MAX_LOGS: usize = 200;
/// A setting change makes the headset play a ~1.2 s voice prompt, and it rejects
/// commands until the prompt ends.
const REJECT_RETRY_DELAY: Duration = Duration::from_millis(1500);

fn main() -> Result<()> {
    let args = AppArgs::parse();
    let mut state = AppState::new();

    if let Some(command) = args.command {
        run_command(command, &mut state, args.json)?;
        return Ok(());
    }

    println!("UGREEN native prototype CLI. Type `help` for commands, `exit` to quit.");
    repl(&mut state)?;
    Ok(())
}

fn repl(state: &mut AppState) -> Result<()> {
    let stdin = io::stdin();
    loop {
        print!("ugreen> ");
        io::stdout().flush()?;

        let mut line = String::new();
        let read = stdin.read_line(&mut line)?;
        if read == 0 {
            println!();
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
            break;
        }
        if trimmed.eq_ignore_ascii_case("help") {
            print_help();
            continue;
        }

        if let Some(parsed) = parse_inline_command(trimmed) {
            if let Err(error) = run_command(parsed.command, state, parsed.json) {
                eprintln!("Error: {error}");
            }
        }
    }

    Ok(())
}

struct ParsedInlineCommand {
    command: Command,
    json: bool,
}

fn parse_inline_command(line: &str) -> Option<ParsedInlineCommand> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let mut args = Vec::with_capacity(tokens.len() + 1);
    args.push("ugreen");
    args.extend(tokens);

    match AppArgs::try_parse_from(args) {
        Ok(parsed) => parsed.command.map(|command| ParsedInlineCommand {
            command,
            json: parsed.json,
        }),
        Err(err) => {
            eprintln!("{err}");
            None
        }
    }
}

fn print_help() {
    println!("UGREEN CLI");
    println!();
    println!("Quick start:");
    println!("  scan");
    println!("  connect max5c");
    println!("  info");
    println!("  anc off");
    println!("  anc transparency");
    println!("  anc light | anc medium | anc deep | anc adaptive");
    println!("  eq bass | eq pop | eq classic");
    println!("  lang english | lang chinese");
    println!("  vol 3");
    println!("  fw");
    println!();
    println!("Tips:");
    println!(
        "  If only one real device is available, commands like `anc`/`eq`/`info` will auto-select it."
    );
    println!("  Device aliases: `max5c`, `max6`, `h6pro`, `t6`.");
    println!("  Add `--json` to any command, for example: `scan --json`, `info --json`.");
    println!("  `factory-reset` is blocked unless you pass `--yes-really-reset-device`.");
    println!();
    println!("Commands:");
    println!("  scan | stop-scan | status");
    println!("  connect <device> | disconnect");
    println!("  info | fw");
    println!("  anc <off|transparency|light|medium|deep|adaptive>");
    println!("  eq <bass|pop|classic|jazz|electronic|folk|rock|treble>");
    println!("  lang <english|chinese>");
    println!("  vol <1-15>");
    println!("  wear <true|false>");
    println!("  game <true|false>");
    println!("  dual <true|false>");
    println!("  spatial <true|false>");
    println!("  hq <true|false>");
    println!("  factory-reset --yes-really-reset-device");
}

fn run_command(command: Command, state: &mut AppState, json_output: bool) -> Result<()> {
    state.ensure_active_device_for_command(&command)?;
    command.execute(state)?;
    // A one-shot run starts from catalog defaults, so re-read the device to report
    // its real state instead of placeholders for everything but the changed field.
    // Best effort: the setting is already applied, and some (LDAC) make the
    // headset reconnect, so a failed read must not turn the command into an error.
    if command.changes_device_setting() {
        if let Err(error) = state.apply_protocol_command(ProtocolCommand::QueryInfo) {
            state.log("warn", "refresh_after_set", json!({ "error": error.to_string() }));
        }
    }
    if json_output {
        state.print_snapshot()?;
    } else {
        state.print_summary(&command);
    }
    Ok(())
}

#[derive(Parser)]
#[command(author, version, about = "UGREEN native protocol shell")]
struct AppArgs {
    /// Print full JSON state after the command
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start scanning for supported earbuds/protocols
    #[command(visible_alias = "s")]
    Scan,
    /// Stop scanning
    #[command(visible_alias = "stop")]
    StopScan,
    /// Print the current state snapshot
    #[command(visible_aliases = ["st", "state"])]
    Status,
    /// Connect to a discovered device
    #[command(visible_aliases = ["conn", "use"])]
    Connect {
        /// Device id (see `scan`)
        device_id: String,
    },
    /// Disconnect the active device
    #[command(visible_aliases = ["disc", "dc"])]
    Disconnect,
    /// Refresh the device info cache
    #[command(visible_aliases = ["info", "qi"])]
    QueryInfo,
    /// Fetch firmware/hardware details
    #[command(visible_aliases = ["fw", "qf"])]
    QueryFirmware,
    /// Update ANC mode (optionally depth)
    #[command(visible_alias = "anc")]
    SetAnc {
        /// Mode (anc/transparency/off)
        mode: String,
        /// Optional ANC depth
        #[arg(short, long)]
        depth: Option<u8>,
    },
    /// Update EQ preset
    #[command(visible_alias = "eq")]
    SetEq {
        /// Mode name (balanced/bass/rock/etc.)
        mode: String,
    },
    /// Update prompt language
    #[command(visible_aliases = ["lang", "voice-lang"])]
    SetPromptLanguage {
        /// Language value
        value: String,
    },
    /// Update prompt volume (1-15)
    #[command(visible_aliases = ["vol", "voice-vol"])]
    SetPromptVolume {
        #[arg(value_parser = clap::value_parser!(u8).range(1..=15))]
        value: u8,
    },
    /// Enable or disable wear detection
    #[command(visible_aliases = ["wear", "ear-detect"])]
    SetWearDetection {
        /// on/off (also true/false, 1/0)
        #[arg(action = clap::ArgAction::Set, value_parser = parse_switch)]
        enabled: bool,
    },
    /// Enable or disable game mode (low latency)
    #[command(visible_aliases = ["game", "game-mode"])]
    SetGameMode {
        /// on/off (also true/false, 1/0)
        #[arg(action = clap::ArgAction::Set, value_parser = parse_switch)]
        enabled: bool,
    },
    /// Enable or disable dual connection (multipoint)
    #[command(visible_aliases = ["dual", "dual-connect", "multipoint"])]
    SetDualConnect {
        /// on/off (also true/false, 1/0)
        #[arg(action = clap::ArgAction::Set, value_parser = parse_switch)]
        enabled: bool,
    },
    /// Enable or disable spatial audio
    #[command(visible_aliases = ["spatial", "spatial-audio"])]
    SetSpatialAudio {
        /// on/off (also true/false, 1/0)
        #[arg(action = clap::ArgAction::Set, value_parser = parse_switch)]
        enabled: bool,
    },
    /// Enable or disable high quality decoding (LDAC)
    #[command(visible_aliases = ["hq", "ldac", "high-quality"])]
    SetHighQuality {
        /// on/off (also true/false, 1/0)
        #[arg(action = clap::ArgAction::Set, value_parser = parse_switch)]
        enabled: bool,
    },
    /// Set what the ANC/left button does per press: none, anc, game, or a raw code
    /// (codes are model-specific; `anc`=8 and `game`=7 are the HiTune Max5 values)
    #[command(visible_aliases = ["buttons", "keys"])]
    SetButtons {
        #[arg(value_parser = parse_button_action)]
        tap: u8,
        #[arg(value_parser = parse_button_action)]
        double_tap: u8,
        #[arg(value_parser = parse_button_action)]
        triple_tap: u8,
        #[arg(value_parser = parse_button_action)]
        long_press: u8,
    },
    /// Perform a factory reset of the active device
    FactoryReset {
        /// Required to unlock destructive reset on a real device
        #[arg(long)]
        yes_really_reset_device: bool,
    },
}

impl Command {
    fn requires_active_device(&self) -> bool {
        matches!(
            self,
            Command::QueryInfo
                | Command::QueryFirmware
                | Command::SetAnc { .. }
                | Command::SetEq { .. }
                | Command::SetPromptLanguage { .. }
                | Command::SetPromptVolume { .. }
                | Command::SetWearDetection { .. }
                | Command::SetGameMode { .. }
                | Command::SetDualConnect { .. }
                | Command::SetSpatialAudio { .. }
                | Command::SetHighQuality { .. }
                | Command::SetButtons { .. }
                | Command::FactoryReset { .. }
        )
    }

    /// Commands that change a setting on the device (everything else only reads).
    fn changes_device_setting(&self) -> bool {
        self.requires_active_device()
            && !matches!(
                self,
                Command::QueryInfo | Command::QueryFirmware | Command::FactoryReset { .. }
            )
    }

    fn execute(&self, state: &mut AppState) -> Result<()> {
        if self.requires_active_device() {
            let device_name = &state.require_active_device()?.device_name;
            if let Some(reason) = unsupported_reason(device_name, self) {
                return Err(anyhow!("{device_name}: {reason}"));
            }
        }
        match self {
            Command::Scan => {
                state.refresh_devices();
                state.scanning = true;
                state.log(
                    "info",
                    "scan_start",
                    json!({ "count": state.devices.len() }),
                );
                println!("Scan started ({}) device(s)", state.devices.len());
            }
            Command::StopScan => {
                state.scanning = false;
                state.log("info", "scan_stop", json!({}));
                println!("Scanning paused");
            }
            Command::Status => {
                println!("Current state:");
            }
            Command::Connect { device_id } => {
                let resolved_device_id = resolve_device_id_alias(device_id);
                let position = state
                    .devices
                    .iter()
                    .position(|item| item.id == resolved_device_id)
                    .ok_or_else(|| anyhow!("Device `{resolved_device_id}` not found"))?;
                let device_name = state.devices[position].device_name.clone();
                let device_id_owned = state.devices[position].id.clone();

                state.active_device_id = Some(device_id_owned.clone());
                state.connected = true;
                state.log("info", "connect", json!({ "deviceId": device_id_owned }));
                println!("Connected to {}", device_name);
            }
            Command::Disconnect => {
                let disconnected = state.active_device_id.take();
                state.connected = false;
                state.log("info", "disconnect", json!({ "deviceId": disconnected }));
                println!("Disconnected");
            }
            Command::QueryInfo => state.apply_protocol_command(ProtocolCommand::QueryInfo)?,
            Command::QueryFirmware => {
                state.apply_protocol_command(ProtocolCommand::QueryFirmware)?
            }
            Command::SetAnc { mode, depth } => {
                let device_name = state.require_active_device()?.device_name.clone();
                state.apply_protocol_command(ProtocolCommand::SetAnc {
                    mode: anc_mode_for_device(&device_name, mode),
                    depth: *depth,
                })?
            }
            Command::SetEq { mode } => {
                state.apply_protocol_command(ProtocolCommand::SetEq { mode: mode.clone() })?
            }
            Command::SetPromptLanguage { value } => {
                state.apply_protocol_command(ProtocolCommand::SetPromptLanguage {
                    value: value.clone(),
                })?
            }
            Command::SetPromptVolume { value } => {
                state.apply_protocol_command(ProtocolCommand::SetPromptVolume { value: *value })?
            }
            Command::SetWearDetection { enabled } => state
                .apply_protocol_command(ProtocolCommand::SetWearDetection { enabled: *enabled })?,
            Command::SetGameMode { enabled } => {
                state.apply_protocol_command(ProtocolCommand::SetGameMode { enabled: *enabled })?
            }
            Command::SetDualConnect { enabled } => {
                state.apply_protocol_command(ProtocolCommand::SetDualConnect { enabled: *enabled })?
            }
            Command::SetSpatialAudio { enabled } => {
                state.apply_protocol_command(ProtocolCommand::SetSpatialAudio { enabled: *enabled })?
            }
            Command::SetHighQuality { enabled } => {
                // Any LDAC write, even of the current value, makes the headset drop and
                // re-establish the audio link (~8 s of silence), so skip no-op writes.
                state.apply_protocol_command(ProtocolCommand::QueryInfo)?;
                if state.require_active_device()?.high_quality == *enabled {
                    state.log("info", "unchanged", json!({ "highQuality": enabled }));
                } else {
                    state.apply_protocol_command(ProtocolCommand::SetHighQuality {
                        enabled: *enabled,
                    })?
                }
            }
            Command::SetButtons {
                tap,
                double_tap,
                triple_tap,
                long_press,
            } => {
                // KEY_SETTING rewrites all 8 slots; keep the right-earbud half as
                // reported, and refuse rather than zero it when the layout is unknown.
                state.apply_protocol_command(ProtocolCommand::QueryInfo)?;
                let mut actions = [0u8; 8];
                let right = state
                    .require_active_device()?
                    .buttons
                    .get(4..8)
                    .ok_or_else(|| anyhow!("device did not report its button layout"))?;
                actions[4..].copy_from_slice(right);
                actions[..4].copy_from_slice(&[*tap, *double_tap, *triple_tap, *long_press]);
                state.apply_protocol_command(ProtocolCommand::SetButtons { actions })?
            }
            Command::FactoryReset {
                yes_really_reset_device,
            } => {
                if !yes_really_reset_device {
                    return Err(anyhow!(
                        "factory-reset is destructive; rerun with `factory-reset --yes-really-reset-device`"
                    ));
                }
                state.apply_protocol_command(ProtocolCommand::FactoryReset)?;
            }
        }
        Ok(())
    }
}

struct AppState {
    devices: Vec<Device>,
    scanning: bool,
    connected: bool,
    active_device_id: Option<String>,
    logs: VecDeque<LogEntry>,
    transport: Box<dyn Transport>,
}

impl AppState {
    fn new() -> Self {
        Self {
            devices: available_devices(),
            scanning: false,
            connected: false,
            active_device_id: None,
            logs: VecDeque::with_capacity(MAX_LOGS),
            transport: Box::new(LinuxTransport::new()),
        }
    }

    fn refresh_devices(&mut self) {
        self.devices = available_devices();
    }

    fn ensure_active_device_for_command(&mut self, command: &Command) -> Result<()> {
        if !command.requires_active_device() || self.active_device_id.is_some() {
            return Ok(());
        }

        self.refresh_devices();
        let selected = if self.devices.len() == 1 {
            self.devices.first()
        } else {
            let mut runtime_devices = self
                .devices
                .iter()
                .filter(|device| device.id.contains(':'))
                .collect::<Vec<_>>();
            if runtime_devices.len() == 1 {
                runtime_devices.pop()
            } else {
                None
            }
        };

        if let Some(device) = selected {
            self.active_device_id = Some(device.id.clone());
            self.connected = true;
            println!("Auto-selected {}", device.device_name);
            self.log("info", "auto_connect", json!({ "deviceId": device.id }));
        }

        Ok(())
    }

    fn require_active_device(&self) -> Result<&Device> {
        let id = self
            .active_device_id
            .as_ref()
            .ok_or_else(|| anyhow!("No active device"))?;
        self.devices
            .iter()
            .find(|device| &device.id == id)
            .ok_or_else(|| anyhow!("Active device `{id}` disappeared"))
    }

    fn require_active_device_mut(&mut self) -> Result<&mut Device> {
        let id = self
            .active_device_id
            .as_ref()
            .ok_or_else(|| anyhow!("No active device"))?;
        self.devices
            .iter_mut()
            .find(|device| &device.id == id)
            .ok_or_else(|| anyhow!("Active device `{id}` disappeared"))
    }

    fn active_protocol_type(&self) -> Result<ProtocolType> {
        Ok(self.require_active_device()?.protocol_type)
    }

    fn active_transport_target(&self) -> Result<TransportTarget> {
        let device = self.require_active_device()?;
        Ok(TransportTarget {
            mac_address: device.mac_address.clone(),
            protocol: device.protocol_type,
            rfcomm_channel: resolve_rfcomm_channel(&device.device_name, &device.mac_address),
        })
    }

    fn apply_protocol_command(&mut self, command: ProtocolCommand) -> Result<()> {
        let protocol_type = self.active_protocol_type()?;
        let target = self.active_transport_target()?;
        let payload = command.payload(protocol_type);
        let mut response = self
            .transport
            .send(&target, &command, &payload)
            .map_err(|error| anyhow!("transport failed: {error}"))?;
        if response.command == UG1_REJECTED {
            self.log(
                "warn",
                "command_rejected",
                json!({ "command": command.name(), "retrying": true }),
            );
            thread::sleep(REJECT_RETRY_DELAY);
            response = self
                .transport
                .send(&target, &command, &payload)
                .map_err(|error| anyhow!("transport failed: {error}"))?;
        }
        if response.command == UG1_REJECTED {
            return Err(anyhow!(
                "headset rejected `{}` (busy with a voice prompt, or unsupported value)",
                command.name()
            ));
        }
        let details = {
            let device = self.require_active_device_mut()?;
            Self::apply_response_to_device(device, &command, &response)
        };
        self.log(
            "info",
            "protocol_command",
            json!({ "command": command.name(), "payload": payload }),
        );
        self.log(
            "info",
            "protocol_response",
            json!({
                "command": response.command,
                "status": if response.success { "ok" } else { "error" },
                "raw": Self::hex_encode(&response.raw),
                "data": response.data,
                "details": details,
            }),
        );
        Ok(())
    }

    fn apply_response_to_device(
        device: &mut Device,
        command: &ProtocolCommand,
        response: &protocol::DecodedFrame,
    ) -> Value {
        if let Some(name) = response.data.get("deviceName").and_then(Value::as_str) {
            device.device_name = name.to_string();
        }
        if let Some(version) = response.data.get("firmwareVersion").and_then(Value::as_str) {
            device.firmware_version = version.to_string();
        }
        if let Some(anc_mode) = response.data.get("ancMode").and_then(Value::as_str) {
            device.anc_mode = anc_mode.to_string();
            device.features.anc = anc_mode != "unknown";
        }
        if let Some(anc_depth) = response.data.get("ancDepth").and_then(Value::as_str) {
            if let Some(depth) = parse_cli_anc_depth(anc_depth) {
                device.anc_depth = depth;
            }
        }
        if let Some(eq_mode) = response.data.get("eqMode").and_then(Value::as_str) {
            device.eq_mode = eq_mode.to_string();
            device.features.eq = true;
        }
        if let Some(language) = response.data.get("promptLanguage").and_then(Value::as_str) {
            device.prompt_language = language.to_string();
        }
        if let Some(volume) = response.data.get("promptVolume").and_then(Value::as_u64) {
            device.prompt_volume = volume as u8;
        }
        if let Some(in_box) = response.data.get("inBox").and_then(Value::as_bool) {
            device.has_box = in_box;
        }
        if let Some(dual_link) = response.data.get("dualLink").and_then(Value::as_bool) {
            device.dual_link = dual_link;
            device.features.dual_link = true;
        }
        if let Some(spatial) = response.data.get("spatialAudio").and_then(Value::as_bool) {
            device.spatial_audio = spatial;
            device.features.spatial_audio = true;
        }
        if let Some(game_mode) = response.data.get("gameMode").and_then(Value::as_bool) {
            device.game_mode = game_mode;
            device.features.game_mode = true;
        }
        if let Some(high_quality) = response.data.get("highQuality").and_then(Value::as_bool) {
            device.high_quality = high_quality;
            device.features.high_quality = true;
        }
        if let Some(audio_share) = response.data.get("audioShare").and_then(Value::as_bool) {
            device.audio_share = audio_share;
            device.features.audio_share = true;
        }
        if let Some(wear_detection) = response.data.get("wearDetection").and_then(Value::as_bool) {
            device.wear_detection = wear_detection;
            device.features.wear_detection = true;
        }
        if let Some(wind_noise) = response.data.get("windNoise").and_then(Value::as_bool) {
            let _ = wind_noise;
            device.features.wind_noise = true;
        }
        if let Some(box_mac) = response.data.get("boxMac").and_then(Value::as_str) {
            device.mac_address = box_mac.to_string();
        }
        if let Some(buttons) = response.data.get("buttons").and_then(Value::as_array) {
            device.buttons = buttons
                .iter()
                .filter_map(|value| value.as_u64().map(|code| code as u8))
                .collect();
        }
        if let Some(battery) = response.data.get("battery") {
            if let Some(left) = battery.get("left").and_then(Value::as_u64) {
                device.battery_left = left as u8;
            }
            if let Some(right) = battery.get("right").and_then(Value::as_u64) {
                device.battery_right = right as u8;
            }
            if let Some(box_level) = battery.get("box").and_then(Value::as_u64) {
                device.battery_box = Some(box_level as u8);
            }
        }
        if response.data.get("findLeft").is_some() || response.data.get("findRight").is_some() {
            device.features.find = true;
        }

        match command {
            ProtocolCommand::QueryInfo | ProtocolCommand::QueryFirmware => response.data.clone(),
            ProtocolCommand::SetAnc { mode, depth } => {
                if let Some(depth_value) = depth {
                    device.anc_depth = *depth_value;
                }
                if depth.is_none() {
                    device.anc_mode = normalize_cli_anc_mode(mode).to_string();
                }
                json!({ "ancMode": device.anc_mode, "ancDepth": device.anc_depth, "success": response.success })
            }
            ProtocolCommand::SetEq { mode } => {
                device.eq_mode = normalize_cli_eq_mode(mode).to_string();
                json!({ "eqMode": device.eq_mode, "success": response.success })
            }
            ProtocolCommand::SetPromptLanguage { value } => {
                if let Some(language) = response.data.get("promptLanguage").and_then(Value::as_str) {
                    device.prompt_language = language.to_string();
                } else {
                    device.prompt_language = value.clone();
                }
                json!({ "promptLanguage": device.prompt_language, "success": response.success })
            }
            ProtocolCommand::SetPromptVolume { value } => {
                if let Some(volume) = response.data.get("promptVolume").and_then(Value::as_u64) {
                    device.prompt_volume = volume as u8;
                } else {
                    device.prompt_volume = *value;
                }
                json!({ "promptVolume": device.prompt_volume, "success": response.success })
            }
            ProtocolCommand::SetWearDetection { enabled } => {
                device.wear_detection = *enabled;
                json!({ "wearDetection": device.wear_detection, "success": response.success })
            }
            ProtocolCommand::SetGameMode { enabled } => {
                device.game_mode = *enabled;
                json!({ "gameMode": device.game_mode, "success": response.success })
            }
            ProtocolCommand::SetDualConnect { enabled } => {
                device.dual_link = *enabled;
                json!({ "dualLink": device.dual_link, "success": response.success })
            }
            ProtocolCommand::SetSpatialAudio { enabled } => {
                device.spatial_audio = *enabled;
                json!({ "spatialAudio": device.spatial_audio, "success": response.success })
            }
            ProtocolCommand::SetHighQuality { enabled } => {
                device.high_quality = *enabled;
                if *enabled {
                    device.dual_link = false;
                    device.game_mode = false;
                }
                json!({
                    "highQuality": device.high_quality,
                    "dualLink": device.dual_link,
                    "gameMode": device.game_mode,
                    "success": response.success
                })
            }
            ProtocolCommand::SetButtons { actions } => {
                device.buttons = actions.to_vec();
                json!({ "buttons": device.buttons, "success": response.success })
            }
            ProtocolCommand::FactoryReset => {
                device.factory_reset();
                json!({ "status": "factory_reset", "success": response.success })
            }
        }
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{:02X}", byte)).collect()
    }

    fn snapshot(&self) -> StateSnapshot {
        let logs = self
            .logs
            .iter()
            .rev()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        StateSnapshot {
            scanning: self.scanning,
            connected: self.connected,
            discovered_devices: self
                .devices
                .iter()
                .map(|device| {
                    let connected = self.active_device_id.as_deref() == Some(&device.id);
                    device.snapshot(connected)
                })
                .collect(),
            active_device: self.active_device().map(|device| device.snapshot(true)),
            logs,
        }
    }

    fn active_device(&self) -> Option<&Device> {
        self.active_device_id
            .as_ref()
            .and_then(|id| self.devices.iter().find(|device| device.id == *id))
    }

    fn log(&mut self, level: &'static str, message: &'static str, details: serde_json::Value) {
        if self.logs.len() >= MAX_LOGS {
            self.logs.pop_front();
        }
        self.logs.push_back(LogEntry {
            ts: Utc::now().to_rfc3339(),
            level: level.to_string(),
            message: message.to_string(),
            details,
        });
    }

    fn print_snapshot(&self) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(&self.snapshot())?);
        Ok(())
    }

    fn print_summary(&self, command: &Command) {
        match command {
            Command::Scan => {
                println!("Found {} device(s):", self.devices.len());
                for device in &self.devices {
                    let active = if self.active_device_id.as_deref() == Some(&device.id) {
                        " [active]"
                    } else {
                        ""
                    };
                    println!("  {} ({}, {}){}", device.device_name, device.id, device.protocol_type.definition().name, active);
                }
            }
            Command::StopScan => println!("Scan stopped."),
            Command::Status => self.print_active_device_summary(),
            Command::Connect { .. } => self.print_active_device_summary(),
            Command::Disconnect => println!("Disconnected."),
            Command::QueryInfo | Command::QueryFirmware => self.print_active_device_summary(),
            Command::SetAnc { .. }
            | Command::SetEq { .. }
            | Command::SetPromptLanguage { .. }
            | Command::SetPromptVolume { .. }
            | Command::SetWearDetection { .. }
            | Command::SetGameMode { .. }
            | Command::SetDualConnect { .. }
            | Command::SetSpatialAudio { .. }
            | Command::SetHighQuality { .. }
            | Command::SetButtons { .. }
            | Command::FactoryReset { .. } => self.print_active_device_summary(),
        }
    }

    fn print_active_device_summary(&self) {
        if let Some(device) = self.active_device() {
            println!("Active device: {} ({})", device.device_name, device.id);
            println!(
                "  ANC: {} (depth {}) | EQ: {} | Prompt: {} @ {}",
                device.anc_mode, device.anc_depth, device.eq_mode, device.prompt_language, device.prompt_volume
            );
            println!(
                "  Battery L/R/Box: {}/{}/{} | FW: {}",
                device.battery_left,
                device.battery_right,
                device
                    .battery_box
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                device.firmware_version
            );
            println!(
                "  Flags: game={} dual={} spatial={} HQ={} wear={}",
                device.game_mode,
                device.dual_link,
                device.spatial_audio,
                device.high_quality,
                device.wear_detection
            );
            if !device.buttons.is_empty() {
                println!("  Buttons: {:?}", device.buttons);
            }
        } else {
            println!("No active device.");
        }
    }
}

#[derive(Serialize)]
struct StateSnapshot {
    scanning: bool,
    connected: bool,
    discovered_devices: Vec<DeviceSnapshot>,
    active_device: Option<DeviceSnapshot>,
    logs: Vec<LogEntry>,
}

#[derive(Clone, Serialize)]
struct LogEntry {
    ts: String,
    level: String,
    message: String,
    details: serde_json::Value,
}

#[derive(Clone)]
struct Device {
    id: String,
    device_name: String,
    device_unique_code: String,
    product_serial_no: String,
    mac_address: String,
    protocol_type: ProtocolType,
    firmware_version: String,
    hardware_version: String,
    battery_left: u8,
    battery_right: u8,
    battery_box: Option<u8>,
    has_box: bool,
    is_over_ear_headphones: bool,
    anc_mode: String,
    anc_depth: u8,
    eq_mode: String,
    prompt_language: String,
    prompt_volume: u8,
    wear_detection: bool,
    game_mode: bool,
    spatial_audio: bool,
    dual_link: bool,
    high_quality: bool,
    audio_share: bool,
    ai_enabled: bool,
    /// ANC/left button actions then right, as in the device state (model-specific codes).
    buttons: Vec<u8>,
    features: FeatureSupport,
}

impl Device {
    fn new(
        id: &'static str,
        device_name: &'static str,
        protocol_type: ProtocolType,
        mac_address: &'static str,
        product_serial_no: &'static str,
        device_unique_code: &'static str,
        battery_left: u8,
        battery_right: u8,
        battery_box: Option<u8>,
        has_box: bool,
        is_over_ear_headphones: bool,
    ) -> Self {
        let definition = protocol_type.definition();
        Self {
            id: id.to_string(),
            device_name: device_name.to_string(),
            device_unique_code: device_unique_code.to_string(),
            product_serial_no: product_serial_no.to_string(),
            mac_address: mac_address.to_string(),
            protocol_type,
            firmware_version: "1.0.7".into(),
            hardware_version: "A1".into(),
            battery_left,
            battery_right,
            battery_box,
            has_box,
            is_over_ear_headphones,
            anc_mode: "anc".into(),
            anc_depth: 2,
            eq_mode: "balanced".into(),
            prompt_language: "Chinese".into(),
            prompt_volume: 8,
            wear_detection: definition.supports.wear_detection,
            game_mode: definition.supports.game_mode,
            spatial_audio: definition.supports.spatial_audio,
            dual_link: definition.supports.dual_link,
            high_quality: definition.supports.high_quality,
            audio_share: definition.supports.audio_share,
            ai_enabled: definition.supports.ai,
            buttons: Vec::new(),
            features: definition.supports.clone(),
        }
    }

    fn snapshot(&self, connected: bool) -> DeviceSnapshot {
        DeviceSnapshot {
            id: self.id.clone(),
            device_name: self.device_name.clone(),
            device_unique_code: self.device_unique_code.clone(),
            product_serial_no: self.product_serial_no.clone(),
            mac_address: self.mac_address.clone(),
            protocol_type: self.protocol_type.id(),
            protocol_name: self.protocol_type.definition().name.to_string(),
            connected,
            firmware_version: self.firmware_version.clone(),
            hardware_version: self.hardware_version.clone(),
            battery_left: self.battery_left,
            battery_right: self.battery_right,
            battery_box: self.battery_box,
            has_box: self.has_box,
            is_over_ear_headphones: self.is_over_ear_headphones,
            anc_mode: self.anc_mode.clone(),
            anc_depth: self.anc_depth,
            eq_mode: self.eq_mode.clone(),
            prompt_language: self.prompt_language.clone(),
            prompt_volume: self.prompt_volume,
            wear_detection: self.wear_detection,
            game_mode: self.game_mode,
            spatial_audio: self.spatial_audio,
            dual_link: self.dual_link,
            high_quality: self.high_quality,
            audio_share: self.audio_share,
            ai_enabled: self.ai_enabled,
            buttons: self.buttons.clone(),
            features: self.features.clone(),
        }
    }

    fn factory_reset(&mut self) {
        let definition = self.protocol_type.definition();
        self.anc_mode = "anc".into();
        self.anc_depth = 2;
        self.eq_mode = "balanced".into();
        self.prompt_language = "Chinese".into();
        self.prompt_volume = 8;
        self.wear_detection = definition.supports.wear_detection;
        self.game_mode = definition.supports.game_mode;
        self.spatial_audio = definition.supports.spatial_audio;
        self.dual_link = definition.supports.dual_link;
        self.high_quality = definition.supports.high_quality;
        self.audio_share = definition.supports.audio_share;
        self.ai_enabled = definition.supports.ai;
        self.buttons.clear();
    }

    fn from_runtime(
        id: String,
        device_name: String,
        protocol_type: ProtocolType,
        mac_address: String,
        product_serial_no: &'static str,
        device_unique_code: &'static str,
        has_box: bool,
        is_over_ear_headphones: bool,
    ) -> Self {
        let definition = protocol_type.definition();
        Self {
            id,
            device_name,
            device_unique_code: device_unique_code.to_string(),
            product_serial_no: product_serial_no.to_string(),
            mac_address,
            protocol_type,
            firmware_version: "unknown".into(),
            hardware_version: "unknown".into(),
            battery_left: 0,
            battery_right: 0,
            battery_box: None,
            has_box,
            is_over_ear_headphones,
            anc_mode: "adaptive".into(),
            anc_depth: 2,
            eq_mode: "classic".into(),
            prompt_language: "Unknown".into(),
            prompt_volume: 8,
            wear_detection: definition.supports.wear_detection,
            game_mode: definition.supports.game_mode,
            spatial_audio: definition.supports.spatial_audio,
            dual_link: definition.supports.dual_link,
            high_quality: definition.supports.high_quality,
            audio_share: definition.supports.audio_share,
            ai_enabled: definition.supports.ai,
            buttons: Vec::new(),
            features: definition.supports.clone(),
        }
    }
}

#[derive(Serialize)]
struct DeviceSnapshot {
    id: String,
    device_name: String,
    device_unique_code: String,
    product_serial_no: String,
    mac_address: String,
    protocol_type: u8,
    protocol_name: String,
    connected: bool,
    firmware_version: String,
    hardware_version: String,
    battery_left: u8,
    battery_right: u8,
    battery_box: Option<u8>,
    has_box: bool,
    is_over_ear_headphones: bool,
    anc_mode: String,
    anc_depth: u8,
    eq_mode: String,
    prompt_language: String,
    prompt_volume: u8,
    wear_detection: bool,
    game_mode: bool,
    spatial_audio: bool,
    dual_link: bool,
    high_quality: bool,
    audio_share: bool,
    ai_enabled: bool,
    /// ANC/left button actions then right, as in the device state (model-specific codes).
    buttons: Vec<u8>,
    features: FeatureSupport,
}

/// Placeholder devices for trying the CLI without a headset (`UGREEN_DEMO=1`).
fn supported_devices() -> Vec<Device> {
    vec![
        Device::new(
            "3C:78:AD:8D:E2:EA",
            "UGREEN HiTune Max5c",
            ProtocolType::Ug1,
            "3C:78:AD:8D:E2:EA",
            "max5c_ota",
            "UGREEN-MAX5C-LIVE",
            0,
            0,
            None,
            false,
            true,
        ),
        Device::new(
            "dev-max6",
            "UGREEN HiTune Max6",
            ProtocolType::Ug3,
            "AA:BB:CC:DD:EE:01",
            "max6_ota",
            "UGREEN-MAX6-001",
            87,
            89,
            None,
            false,
            true,
        ),
        Device::new(
            "dev-h6pro",
            "UGREEN HiTune H6 Pro",
            ProtocolType::Ug2,
            "AA:BB:CC:DD:EE:02",
            "h6_pro_ota",
            "UGREEN-H6PRO-001",
            76,
            78,
            Some(92),
            true,
            false,
        ),
        Device::new(
            "dev-t6",
            "UGREEN HiTune T6",
            ProtocolType::Ug1,
            "AA:BB:CC:DD:EE:03",
            "t6_ota",
            "UGREEN-T6-001",
            61,
            64,
            Some(81),
            true,
            false,
        ),
    ]
}

/// Connected headsets only. The demo catalog used to be the fallback, which made
/// commands silently target a fake Max5c while the real headset was reconnecting;
/// it is now opt-in via `UGREEN_DEMO=1`.
fn available_devices() -> Vec<Device> {
    match detect_linux_devices() {
        Ok(devices) => devices,
        Err(_) if std::env::var_os("UGREEN_DEMO").is_some_and(|value| value == "1") => {
            supported_devices()
        }
        Err(_) => Vec::new(),
    }
}

fn detect_linux_devices() -> Result<Vec<Device>> {
    let output = ProcessCommand::new("bluetoothctl")
        .args(["devices", "Connected"])
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("bluetoothctl devices Connected failed"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = stdout
        .lines()
        .filter_map(parse_bluetoothctl_device_line)
        .filter_map(|(mac, name)| catalog_device_for_name(&name, &mac))
        .collect::<Vec<_>>();

    if devices.is_empty() {
        return Err(anyhow!("no supported connected devices found"));
    }

    devices.sort_by(|left, right| left.device_name.cmp(&right.device_name));
    Ok(devices)
}

/// RFCOMM channel of the control port. Models differ (Max5c uses 1, Max5 uses 12):
/// verified models keep their channel, others are looked up over SDP.
/// `UGREEN_RFCOMM_CHANNEL` overrides both.
fn resolve_rfcomm_channel(device_name: &str, mac_address: &str) -> u8 {
    if let Some(channel) = std::env::var("UGREEN_RFCOMM_CHANNEL")
        .ok()
        .and_then(|value| value.trim().parse().ok())
    {
        return channel;
    }
    if device_name == "UGREEN HiTune Max5c" {
        return 1;
    }
    discover_serial_port_channel(mac_address).unwrap_or(1)
}

fn discover_serial_port_channel(mac_address: &str) -> Option<u8> {
    // A targeted Serial Port search takes ~0.1 s; `sdptool records` walks every
    // record handle and takes ~30 s.
    let output = ProcessCommand::new("sdptool")
        .args(["search", "--bdaddr", mac_address, "SP"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_serial_port_channel(&String::from_utf8_lossy(&output.stdout))
}

/// First Serial Port record that is not the OTA service (vendor UUID 6666...).
/// Records split on "Service RecHandle:"; each name line lands in the previous chunk,
/// which is harmless since only UUIDs and the channel are inspected.
fn parse_serial_port_channel(sdp_records: &str) -> Option<u8> {
    const SERIAL_PORT_UUID: &str = "00001101-0000-1000-8000-00805f9b34fb";
    sdp_records
        .split("Service RecHandle:")
        .skip(1)
        .filter(|record| {
            (record.contains("(0x1101)") || record.contains(SERIAL_PORT_UUID))
                && !record.contains("66666666-")
        })
        .find_map(|record| {
            record
                .lines()
                .find_map(|line| line.trim().strip_prefix("Channel:"))
                .and_then(|channel| channel.trim().parse().ok())
        })
}

fn parse_bluetoothctl_device_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let remainder = trimmed.strip_prefix("Device ")?;
    let mut parts = remainder.splitn(2, ' ');
    let mac = parts.next()?.trim().to_string();
    let name = parts.next()?.trim().to_string();
    Some((mac, name))
}

fn catalog_device_for_name(name: &str, mac_address: &str) -> Option<Device> {
    let normalized = name.trim();
    let (protocol_type, product_serial_no, device_unique_code, is_over_ear, has_box) =
        match normalized {
            "UGREEN HiTune Max5c" => (
                ProtocolType::Ug1,
                "max5c_ota",
                "UGREEN-MAX5C-LIVE",
                true,
                false,
            ),
            "UGREEN HiTune Max5" => (
                ProtocolType::Ug1,
                "max5_ota",
                "UGREEN-MAX5-LIVE",
                true,
                false,
            ),
            "UGREEN HiTune H6 Pro" => (
                ProtocolType::Ug2,
                "h6_pro_ota",
                "UGREEN-H6PRO-LIVE",
                false,
                true,
            ),
            "UGREEN HiTune Max6" => (
                ProtocolType::Ug3,
                "max6_ota",
                "UGREEN-MAX6-LIVE",
                true,
                false,
            ),
            "UGREEN HiTune T6" => (ProtocolType::Ug1, "t6_ota", "UGREEN-T6-LIVE", false, true),
            _ => return None,
        };

    Some(Device::from_runtime(
        mac_address.to_string(),
        normalized.to_string(),
        protocol_type,
        mac_address.to_string(),
        product_serial_no,
        device_unique_code,
        has_box,
        is_over_ear,
    ))
}

fn normalize_cli_eq_mode(mode: &str) -> &'static str {
    match mode.to_ascii_lowercase().as_str() {
        "jazz" => "jazz",
        "electronic" => "electronic",
        "pop" | "popular" => "pop",
        "classical" => "classical",
        "rock" => "rock",
        "bass" => "bass",
        "treble" => "treble",
        "original" => "original",
        "balanced" | "classic" | "default" => "classic",
        _ => "classic",
    }
}

/// Button action names as offered by the official app for the HiTune Max5.
/// Codes are firmware-specific (8 toggles EQ on other models), so raw codes pass through.
fn parse_button_action(value: &str) -> Result<u8, String> {
    match value.to_ascii_lowercase().as_str() {
        "none" | "off" => Ok(0),
        "game" | "game-mode" => Ok(7),
        "anc" | "noise" | "noise-control" => Ok(8),
        other => other
            .parse()
            .map_err(|_| format!("expected none/anc/game or a number, got `{value}`")),
    }
}

/// A plain `bool` positional is a value-less flag in clap, so toggles could only
/// ever be switched on. Parse an explicit value instead.
fn parse_switch(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Ok(true),
        "off" | "false" | "0" | "no" => Ok(false),
        _ => Err(format!("expected on/off, got `{value}`")),
    }
}

/// Commands a model does not support, per the official app's model config. The
/// headset never answers them, so they are refused up front instead of waiting out
/// the transport timeout.
fn unsupported_reason(device_name: &str, command: &Command) -> Option<&'static str> {
    if device_name != "UGREEN HiTune Max5" {
        return None;
    }
    match command {
        Command::SetPromptVolume { .. } => Some("prompt volume is not supported"),
        Command::SetSpatialAudio { .. } => Some("spatial audio is not supported"),
        Command::SetWearDetection { .. } => Some("wear detection is not supported"),
        _ => None,
    }
}

/// Max5 uses its own values for off/transparency and has no adaptive ANC, so
/// translate the generic CLI modes for it; other models keep the mode as given.
fn anc_mode_for_device(device_name: &str, mode: &str) -> String {
    if device_name != "UGREEN HiTune Max5" {
        return mode.to_string();
    }
    // ANC depth lives in the high nibble: light 0xC1, medium 0xB1, deep 0xA1.
    match normalize_cli_anc_mode(mode) {
        "off" => "basic-off",
        "transparent" => "basic-transparent",
        "light" => "light",
        "medium" => "medium",
        _ => "deep",
    }
    .to_string()
}

fn normalize_cli_anc_mode(mode: &str) -> &'static str {
    match mode.to_ascii_lowercase().as_str() {
        "off" => "off",
        "transparency" | "transparent" => "transparent",
        "light" => "light",
        "medium" => "medium",
        "deep" => "deep",
        _ => "adaptive",
    }
}

fn resolve_device_id_alias(device_id: &str) -> &str {
    match device_id.to_ascii_lowercase().as_str() {
        "max5c" | "max-5c" | "hitune-max5c" => "3C:78:AD:8D:E2:EA",
        "max6" | "max-6" | "hitune-max6" => "dev-max6",
        "h6pro" | "h6-pro" => "dev-h6pro",
        "t6" => "dev-t6",
        _ => device_id,
    }
}

fn parse_cli_anc_depth(depth: &str) -> Option<u8> {
    match depth.to_ascii_lowercase().as_str() {
        "off" => Some(0),
        "transparent" | "transparency" | "adaptive" | "auto" => Some(2),
        "light" => Some(1),
        "medium" => Some(2),
        "deep" => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `sdptool search --bdaddr <mac> SP` output from a HiTune Max5.
    const MAX5_SDP_SERIAL_PORT: &str = r#"Searching for SP on 98:47:44:6A:87:47 ...
Service Name: UGREEN HiTune Max Lite
Service RecHandle: 0x10004
Service Class ID List:
  UUID 128: 00001101-0000-1000-8000-00805f9b34fb
Protocol Descriptor List:
  "L2CAP" (0x0100)
  "RFCOMM" (0x0003)
    Channel: 12
Profile Descriptor List:
  "Serial Port" (0x1101)
    Version: 0x0102

Service Name: UGREEN HiTune Max Lite
Service RecHandle: 0x10005
Service Class ID List:
  UUID 128: 00001101-0000-1000-8000-00805f9b34fb
Protocol Descriptor List:
  "L2CAP" (0x0100)
  "RFCOMM" (0x0003)
    Channel: 18
Profile Descriptor List:
  "Serial Port" (0x00001101-0000-1000-8000-00805f9b34fb)
    Version: 0x0102

Service Name: OTA1
Service RecHandle: 0x10006
Service Class ID List:
  UUID 128: 66666666-6666-6666-6666-666666666666
Protocol Descriptor List:
  "L2CAP" (0x0100)
  "RFCOMM" (0x0003)
    Channel: 13
Profile Descriptor List:
  "Serial Port" (0x1101)
    Version: 0x0102
"#;

    #[test]
    fn serial_port_channel_skips_ota_record() {
        assert_eq!(parse_serial_port_channel(MAX5_SDP_SERIAL_PORT), Some(12));
    }

    #[test]
    fn serial_port_channel_accepts_long_form_uuid() {
        let second_only = MAX5_SDP_SERIAL_PORT
            .split("Service RecHandle: 0x10005")
            .nth(1)
            .map(|rest| format!("Service RecHandle: 0x10005{rest}"))
            .unwrap();
        assert_eq!(parse_serial_port_channel(&second_only), Some(18));
    }

    #[test]
    fn serial_port_channel_ignores_ota_only_output() {
        let ota_only = MAX5_SDP_SERIAL_PORT
            .split("Service Name: OTA1")
            .nth(1)
            .map(|rest| format!("Service Name: OTA1{rest}"))
            .unwrap();
        assert_eq!(parse_serial_port_channel(&ota_only), None);
        assert_eq!(parse_serial_port_channel(""), None);
    }

    #[test]
    fn max5c_keeps_its_verified_channel() {
        assert_eq!(resolve_rfcomm_channel("UGREEN HiTune Max5c", "00:00:00:00:00:00"), 1);
    }

    #[test]
    fn switches_accept_explicit_values() {
        assert_eq!(parse_switch("on"), Ok(true));
        assert_eq!(parse_switch("FALSE"), Ok(false));
        assert_eq!(parse_switch("0"), Ok(false));
        assert!(parse_switch("maybe").is_err());
    }

    #[test]
    fn toggles_can_be_switched_off() {
        let args = AppArgs::try_parse_from(["ugreen_core", "game", "off"]).unwrap();
        assert!(matches!(args.command, Some(Command::SetGameMode { enabled: false })));
    }

    #[test]
    fn button_actions_accept_names_and_codes() {
        assert_eq!(parse_button_action("none"), Ok(0));
        assert_eq!(parse_button_action("anc"), Ok(8));
        assert_eq!(parse_button_action("game"), Ok(7));
        assert_eq!(parse_button_action("1"), Ok(1));
        assert!(parse_button_action("ldac").is_err());
    }

    #[test]
    fn max5_refuses_unsupported_commands() {
        let max5 = "UGREEN HiTune Max5";
        assert!(unsupported_reason(max5, &Command::SetPromptVolume { value: 8 }).is_some());
        assert!(unsupported_reason(max5, &Command::SetSpatialAudio { enabled: false }).is_some());
        assert!(unsupported_reason(max5, &Command::SetGameMode { enabled: false }).is_none());
        let max5c = "UGREEN HiTune Max5c";
        assert!(unsupported_reason(max5c, &Command::SetPromptVolume { value: 8 }).is_none());
    }

    #[test]
    fn max5_anc_modes_use_its_own_values() {
        assert_eq!(anc_mode_for_device("UGREEN HiTune Max5", "off"), "basic-off");
        assert_eq!(anc_mode_for_device("UGREEN HiTune Max5", "transparency"), "basic-transparent");
        assert_eq!(anc_mode_for_device("UGREEN HiTune Max5", "medium"), "medium");
        assert_eq!(anc_mode_for_device("UGREEN HiTune Max5", "anc"), "deep");
        assert_eq!(anc_mode_for_device("UGREEN HiTune Max5c", "off"), "off");
    }
}
