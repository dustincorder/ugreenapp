use anyhow::{Context, Result, anyhow};
use libc::{
    POLLIN, SO_RCVTIMEO, SO_SNDTIMEO, SOCK_STREAM, SOL_SOCKET, c_void, pollfd, suseconds_t, time_t,
    timeval,
};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Write};
use std::mem::size_of;
use std::os::fd::{AsRawFd, FromRawFd};
use std::thread;
use std::time::Duration;

use crate::protocol::{
    DecodedFrame, ProtocolCommand, ProtocolFacade, ProtocolType, UgProtocolFacade,
    find_complete_rcsp_frame, find_complete_ug1_frame, find_ug1_reject_frame, unwrap_rcsp_custom,
    wrap_rcsp_custom,
};

pub struct TransportTarget {
    pub mac_address: String,
    pub protocol: ProtocolType,
    pub rfcomm_channel: u8,
}

pub trait Transport {
    fn send(
        &mut self,
        target: &TransportTarget,
        command: &ProtocolCommand,
        payload: &Value,
    ) -> Result<DecodedFrame>;
}

pub struct LinuxTransport {
    facade: Box<dyn ProtocolFacade>,
    sequence: u8,
}

impl LinuxTransport {
    pub fn new() -> Self {
        Self {
            facade: Box::new(UgProtocolFacade::new()),
            sequence: 0,
        }
    }

    fn next_sequence(&mut self) -> u8 {
        let seq = self.sequence;
        self.sequence = self.sequence.wrapping_add(1) % 16;
        seq
    }

    fn connect_rfcomm(&self, target: &TransportTarget) -> Result<File> {
        const AF_BLUETOOTH: i32 = 31;
        const BTPROTO_RFCOMM: i32 = 3;
        const SOL_BLUETOOTH: i32 = 274;
        const SOL_RFCOMM: i32 = 18;
        const BT_SECURITY: i32 = 4;
        const BT_SECURITY_HIGH: u8 = 3;
        const RFCOMM_LM: i32 = 0x03;
        const RFCOMM_LM_AUTH: i32 = 0x0002;
        const RFCOMM_LM_ENCRYPT: i32 = 0x0004;
        const RFCOMM_LM_SECURE: i32 = 0x0020;

        #[repr(C)]
        struct SockAddrRc {
            rc_family: libc::sa_family_t,
            rc_bdaddr: [u8; 6],
            rc_channel: u8,
        }

        #[repr(C)]
        struct BtSecurity {
            level: u8,
            key_size: u8,
        }

        let mut last_error = None;
        for attempt in 0..3 {
            let fd = unsafe { libc::socket(AF_BLUETOOTH, SOCK_STREAM, BTPROTO_RFCOMM) };
            if fd < 0 {
                last_error = Some(anyhow!("failed to create RFCOMM socket"));
                continue;
            }

            let mut close_fd = true;
            let result = (|| {
                set_socket_timeout(fd, Duration::from_secs(3))?;
                let security = BtSecurity {
                    level: BT_SECURITY_HIGH,
                    key_size: 0,
                };
                let rc = unsafe {
                    libc::setsockopt(
                        fd,
                        SOL_BLUETOOTH,
                        BT_SECURITY,
                        &security as *const BtSecurity as *const c_void,
                        size_of::<BtSecurity>() as libc::socklen_t,
                    )
                };
                if rc < 0 {
                    return Err(anyhow!("failed to raise Bluetooth RFCOMM security"));
                }

                let link_mode = RFCOMM_LM_AUTH | RFCOMM_LM_ENCRYPT | RFCOMM_LM_SECURE;
                let rc = unsafe {
                    libc::setsockopt(
                        fd,
                        SOL_RFCOMM,
                        RFCOMM_LM,
                        &link_mode as *const i32 as *const c_void,
                        size_of::<i32>() as libc::socklen_t,
                    )
                };
                if rc < 0 {
                    return Err(anyhow!("failed to enable RFCOMM auth/encryption"));
                }

                let address = SockAddrRc {
                    rc_family: AF_BLUETOOTH as libc::sa_family_t,
                    rc_bdaddr: parse_bluetooth_address(&target.mac_address)?,
                    rc_channel: target.rfcomm_channel,
                };
                let rc = unsafe {
                    libc::connect(
                        fd,
                        &address as *const SockAddrRc as *const libc::sockaddr,
                        size_of::<SockAddrRc>() as libc::socklen_t,
                    )
                };
                if rc < 0 {
                    return Err(anyhow!(
                        "RFCOMM connect to {} channel {} failed",
                        target.mac_address,
                        target.rfcomm_channel
                    ));
                }

                close_fd = false;
                Ok(unsafe { File::from_raw_fd(fd) })
            })();

            if close_fd {
                unsafe {
                    libc::close(fd);
                }
            }

            match result {
                Ok(file) => return Ok(file),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 2 {
                        thread::sleep(Duration::from_millis(350));
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("RFCOMM connect failed")))
    }
}

impl Transport for LinuxTransport {
    fn send(
        &mut self,
        target: &TransportTarget,
        command: &ProtocolCommand,
        payload: &Value,
    ) -> Result<DecodedFrame> {
        let seq = self.next_sequence();
        let inner = self
            .facade
            .encode_inner(target.protocol, command, payload, seq)
            .context("failed to encode protocol packet")?;
        let mut socket = self.connect_rfcomm(target)?;

        match self.send_direct(&mut socket, target, command, &inner) {
            Ok(frame) => Ok(frame),
            Err(direct_error) => match self.send_rcsp(&mut socket, target, &inner, seq) {
                Ok(frame) => Ok(frame),
                Err(rcsp_error) => Err(anyhow!(
                    "direct failed: {direct_error}; rcsp failed: {rcsp_error}"
                )),
            },
        }
    }
}

impl LinuxTransport {
    fn send_rcsp(
        &mut self,
        socket: &mut File,
        target: &TransportTarget,
        inner: &[u8],
        seq: u8,
    ) -> Result<DecodedFrame> {
        let rcsp = wrap_rcsp_custom(inner, seq);
        socket
            .write_all(&rcsp)
            .context("failed to write RFCOMM RCSP packet")?;
        socket.flush().ok();

        let raw_response = read_complete_rcsp_frame(socket)?;
        let inner_response = unwrap_rcsp_custom(&raw_response)?;
        let mut decoded = self
            .facade
            .decode_inner(target.protocol, &inner_response)
            .context("failed to decode RCSP device response")?;
        decoded.raw = raw_response;
        Ok(decoded)
    }

    fn send_direct(
        &mut self,
        socket: &mut File,
        target: &TransportTarget,
        command: &ProtocolCommand,
        inner: &[u8],
    ) -> Result<DecodedFrame> {
        socket
            .write_all(inner)
            .context("failed to write direct protocol packet")?;
        socket.flush().ok();

        if target.protocol != ProtocolType::Ug1
            && matches!(
            command,
            ProtocolCommand::SetAnc { .. }
                | ProtocolCommand::SetEq { .. }
                | ProtocolCommand::SetPromptLanguage { .. }
                | ProtocolCommand::SetPromptVolume { .. }
                | ProtocolCommand::FactoryReset
        )
        {
            return Ok(DecodedFrame {
                success: true,
                command: command.name().to_string(),
                raw: inner.to_vec(),
                data: serde_json::json!({ "status": "sent" }),
            });
        }

        let raw_response = read_complete_direct_frame(socket, target.protocol)?;
        let mut decoded = self
            .facade
            .decode_inner(target.protocol, &raw_response)
            .context("failed to decode direct device response")?;
        decoded.raw = raw_response;
        Ok(decoded)
    }
}

fn read_complete_rcsp_frame(socket: &mut File) -> Result<Vec<u8>> {
    let fd = socket.as_raw_fd();
    let mut buffer = Vec::with_capacity(512);
    let mut chunk = [0u8; 256];

    for _ in 0..8 {
        let mut poll = pollfd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 1500) };
        if ready < 0 {
            return Err(anyhow!("RFCOMM poll failed"));
        }
        if ready == 0 {
            continue;
        }
        let read = socket
            .read(&mut chunk)
            .context("failed to read RFCOMM data")?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(frame) = find_complete_rcsp_frame(&buffer) {
            return Ok(frame);
        }
    }

    Err(anyhow!(
        "device did not send a complete RCSP response; partial={}",
        buffer
            .iter()
            .map(|byte| format!("{:02X}", byte))
            .collect::<String>()
    ))
}

fn read_complete_direct_frame(socket: &mut File, protocol: ProtocolType) -> Result<Vec<u8>> {
    let fd = socket.as_raw_fd();
    let mut buffer = Vec::with_capacity(256);
    let mut chunk = [0u8; 256];

    for _ in 0..8 {
        let mut poll = pollfd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 1200) };
        if ready < 0 {
            return Err(anyhow!("direct protocol poll failed"));
        }
        if ready == 0 {
            // Nothing more is coming: a short reject frame is the whole answer.
            if protocol == ProtocolType::Ug1 {
                if let Some(frame) = find_ug1_reject_frame(&buffer) {
                    return Ok(frame);
                }
            }
            continue;
        }

        let read = socket
            .read(&mut chunk)
            .context("failed to read direct protocol data")?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        match protocol {
            ProtocolType::Ug1 => {
                if let Some(frame) = find_complete_ug1_frame(&buffer) {
                    return Ok(frame);
                }
            }
            _ => {
                if buffer.len() >= 5 {
                    let total = 5 + buffer[4] as usize;
                    if buffer.len() >= total {
                        return Ok(buffer[..total].to_vec());
                    }
                }
            }
        }
    }

    Err(anyhow!(
        "device did not send a complete direct response; partial={}",
        buffer
            .iter()
            .map(|byte| format!("{:02X}", byte))
            .collect::<String>()
    ))
}

fn set_socket_timeout(fd: i32, duration: Duration) -> Result<()> {
    let timeout = timeval {
        tv_sec: duration.as_secs() as time_t,
        tv_usec: duration.subsec_micros() as suseconds_t,
    };
    let result = unsafe {
        libc::setsockopt(
            fd,
            SOL_SOCKET,
            SO_RCVTIMEO,
            &timeout as *const timeval as *const c_void,
            size_of::<timeval>() as libc::socklen_t,
        )
    };
    if result < 0 {
        return Err(anyhow!("failed to set receive timeout"));
    }
    let result = unsafe {
        libc::setsockopt(
            fd,
            SOL_SOCKET,
            SO_SNDTIMEO,
            &timeout as *const timeval as *const c_void,
            size_of::<timeval>() as libc::socklen_t,
        )
    };
    if result < 0 {
        return Err(anyhow!("failed to set send timeout"));
    }
    Ok(())
}

fn parse_bluetooth_address(value: &str) -> Result<[u8; 6]> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err(anyhow!("invalid bluetooth address `{value}`"));
    }

    let mut address = [0u8; 6];
    for (index, part) in parts.iter().rev().enumerate() {
        address[index] = u8::from_str_radix(part, 16)
            .with_context(|| format!("invalid bluetooth address `{value}`"))?;
    }
    Ok(address)
}
