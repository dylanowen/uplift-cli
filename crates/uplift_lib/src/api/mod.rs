mod command;
mod message;

use btleplug::api::bleuuid;
pub use command::*;
pub use message::*;
use uuid::Uuid;

pub const DESK_SERVICE_UUID: Uuid = bleuuid::uuid_from_u16(0xff12);

pub const DESK_DATA_IN_UUID: Uuid = bleuuid::uuid_from_u16(0xff01);
pub const DESK_DATA_OUT_UUID: Uuid = bleuuid::uuid_from_u16(0xff02);
pub const DESK_NAME_UUID: Uuid = bleuuid::uuid_from_u16(0xff06);

/// Denotes the end of a message or command
const PAYLOAD_END: u8 = 0x7e;

#[inline]
const fn serialize_height(height: u16) -> [u8; 2] {
    height.to_be_bytes()
}

#[inline]
const fn deserialize_height(raw_height: &[u8]) -> u16 {
    u16::from_be_bytes([raw_height[0], raw_height[1]])
}

#[inline]
fn serialize_payload(prefix: &[u8], cmd: u8, data: &[u8]) -> Vec<u8> {
    let data_len = data.len() as u8;
    // prefix_length + cmd + len + data + checksum + payload_end
    let mut payload = Vec::with_capacity(prefix.len() + 1 + 1 + data_len as usize + 1 + 1);
    payload.extend_from_slice(prefix);

    // since it's a checksum assume we just wrap when adding
    let mut checksum = cmd.wrapping_add(data_len);
    payload.push(cmd);
    payload.push(data_len);
    for d in data {
        payload.push(*d);
        checksum = checksum.wrapping_add(*d);
    }
    payload.push(checksum);
    payload.push(PAYLOAD_END);

    payload
}
