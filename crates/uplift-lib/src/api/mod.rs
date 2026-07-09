mod command;
mod message;

use btleplug::api::bleuuid;
pub use command::*;
pub use message::*;
use uuid::Uuid;

#[derive(Copy, Clone, Debug)]
pub struct DeskUuids {
    pub service_uuid: Uuid,
    pub data_in: Uuid,
    pub data_out: Uuid,
    pub descriptors_out_uuid: Uuid,
    pub name_uuid: Uuid,
}

const DESK_1: DeskUuids = DeskUuids {
    service_uuid: bleuuid::uuid_from_u16(0xff12),
    data_in: bleuuid::uuid_from_u16(0xff01),
    data_out: bleuuid::uuid_from_u16(0xff02),
    descriptors_out_uuid: bleuuid::uuid_from_u16(0x2902),
    name_uuid: bleuuid::uuid_from_u16(0xff06),
};

const DESK_3: DeskUuids = DeskUuids {
    service_uuid: bleuuid::uuid_from_u16(0xfe60),
    data_in: bleuuid::uuid_from_u16(0xfe61),
    data_out: bleuuid::uuid_from_u16(0xfe62),
    descriptors_out_uuid: bleuuid::uuid_from_u16(0x2902),
    name_uuid: bleuuid::uuid_from_u16(0xfe63),
};

const DESK_4: DeskUuids = DeskUuids {
    service_uuid: bleuuid::uuid_from_u16(0xff00),
    data_in: bleuuid::uuid_from_u16(0xff01),
    data_out: bleuuid::uuid_from_u16(0xff02),
    descriptors_out_uuid: bleuuid::uuid_from_u16(0x2902),
    name_uuid: bleuuid::uuid_from_u16(0xfe63),
};

const DESK_5: DeskUuids = DeskUuids {
    service_uuid: bleuuid::uuid_from_u16(0x00ff),
    data_in: bleuuid::uuid_from_u16(0x01ff),
    data_out: bleuuid::uuid_from_u16(0x02ff),
    descriptors_out_uuid: bleuuid::uuid_from_u16(0x2902),
    name_uuid: bleuuid::uuid_from_u16(0x36ef),
};

pub const DESK_UUIDS: [DeskUuids; 4] = [DESK_1, DESK_3, DESK_4, DESK_5];

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
