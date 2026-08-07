// Minecraft "Server List Ping" protocol (modern, 1.7+), implemented
// directly — this is what lets the launcher show server online/offline
// and player count without needing any backend/website.
//
// Protocol: https://wiki.vg/Server_List_Ping
//   1. Open TCP connection.
//   2. Send a Handshake packet (state=1 "status").
//   3. Send an empty Status Request packet.
//   4. Read back a Status Response packet containing a JSON string.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

async fn read_varint(stream: &mut TcpStream) -> Result<i32> {
    let mut result: i32 = 0;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        result |= ((byte[0] & 0x7F) as i32) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 32 {
            return Err(anyhow!("VarInt too long"));
        }
    }
    Ok(result)
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

#[derive(Debug, Deserialize)]
struct SlpPlayers {
    online: u32,
    max: u32,
}

#[derive(Debug, Deserialize)]
struct SlpResponse {
    players: Option<SlpPlayers>,
}

pub struct PingResult {
    pub online: bool,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
}

pub async fn ping_server(host: &str, port: u16) -> PingResult {
    match timeout(Duration::from_secs(4), ping_inner(host, port)).await {
        Ok(Ok(result)) => result,
        _ => PingResult {
            online: false,
            players_online: None,
            players_max: None,
        },
    }
}

async fn ping_inner(host: &str, port: u16) -> Result<PingResult> {
    let mut stream = TcpStream::connect((host, port)).await?;

    // Handshake packet (id 0x00): protocol_version, address, port, next_state=1
    let mut handshake_body = Vec::new();
    write_varint(&mut handshake_body, 0x00);
    write_varint(&mut handshake_body, -1); // protocol version: unknown, servers accept -1/any
    write_string(&mut handshake_body, host);
    handshake_body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake_body, 1); // next state: status

    let mut handshake_packet = Vec::new();
    write_varint(&mut handshake_packet, handshake_body.len() as i32);
    handshake_packet.extend_from_slice(&handshake_body);
    stream.write_all(&handshake_packet).await?;

    // Status Request packet (id 0x00, empty body)
    let mut status_request = Vec::new();
    write_varint(&mut status_request, 1); // length = 1 (just the packet id)
    status_request.push(0x00);
    stream.write_all(&status_request).await?;

    // Read Status Response: [length varint][packet id varint][json string]
    let _packet_len = read_varint(&mut stream).await?;
    let _packet_id = read_varint(&mut stream).await?;
    let json_len = read_varint(&mut stream).await? as usize;
    let mut json_buf = vec![0u8; json_len];
    stream.read_exact(&mut json_buf).await?;
    let json_str = String::from_utf8_lossy(&json_buf);

    let parsed: SlpResponse = serde_json::from_str(&json_str)
        .map_err(|e| anyhow!("Не удалось разобрать ответ сервера: {}", e))?;

    Ok(PingResult {
        online: true,
        players_online: parsed.players.as_ref().map(|p| p.online),
        players_max: parsed.players.as_ref().map(|p| p.max),
    })
}
