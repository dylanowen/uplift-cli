use crate::api::{deserialize_height, PAYLOAD_END};
use nom::branch::alt;
use nom::bytes::complete::{tag, take};
use nom::multi::many0;
use nom::IResult;
use nom::Parser;
use std::fmt::{Debug, Formatter};

type NomResult<'a, O> = Result<O, nom::Err<nom::error::Error<&'a [u8]>>>;

const MESSAGE_PREFIX: [u8; 2] = [0xf2, 0xf2];

const HEIGHT_CODE: u8 = 0x01;
const PHYSICAL_LIMITS_CODE: u8 = 0x07;
#[allow(dead_code)]
const ID_CODE: u8 = 0x0f; // not sure what this does but it seems to be related to iD in Calibration?
const HEIGHT_UNIT_CODE: u8 = 0x0e;
#[allow(dead_code)]
const IA_CODE: u8 = 0x10; // Probably stores a height, maybe some minimum?
const LIMITED_STATUS_CODE: u8 = 0x20;
#[allow(dead_code)]
const HEIGHT_MAX_CODE: u8 = 0x21;
#[allow(dead_code)]
const HEIGHT_MIN_CODE: u8 = 0x22;

#[derive(Eq, PartialEq, Clone)]
pub enum Message {
    Height(u16),
    PhysicalLimits { min: u16, max: u16 },
    HeightUnit(HeightUnit),
    LimitedStatus, // TODO this has more data but it's unused
    Unknown { code: u8, data: Vec<u8> },
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum HeightUnit {
    Cm,
    Inch,
}

impl Message {
    pub fn parse(raw_message: &[u8]) -> IResult<&[u8], Vec<Message>> {
        many0(
            (
                tag(MESSAGE_PREFIX.as_ref()),
                parse_message_data,
                tag([PAYLOAD_END].as_ref()),
            )
                .map(|(_, message, _)| message),
        )
        .parse(raw_message)
    }
}

fn parse_message_data(initial_i: &[u8]) -> IResult<&[u8], Message> {
    let (i, cmd_len) = take(2usize)(initial_i)?;
    let cmd = cmd_len[0];
    let len = cmd_len[1];
    let (i, data) = take(len)(i)?;
    let computed_checksum = data
        .iter()
        .fold(cmd.wrapping_add(len), |c, d| c.wrapping_add(*d));
    let (i, _) = tag([computed_checksum].as_ref())(i)?;

    let message = match cmd {
        HEIGHT_CODE => parse_height_message(data)?,
        HEIGHT_UNIT_CODE => parse_height_unit_message(data)?,
        PHYSICAL_LIMITS_CODE => parse_physical_limits_message(data)?,
        LIMITED_STATUS_CODE => parse_limited_status_message(data)?,
        _ => Message::Unknown {
            code: cmd,
            data: data.into(),
        },
    };

    Ok((i, message))
}

fn parse_height_message(data: &[u8]) -> NomResult<'_, Message> {
    // TODO what does the bonus byte mean? It always seems to be 0x03
    let (_, data) = take(2usize)(data)?;

    let height = deserialize_height(data);

    Ok(Message::Height(height))
}

fn parse_height_unit_message(data: &[u8]) -> NomResult<'_, Message> {
    let (_, height_unit) = alt((
        tag([0x00].as_ref()).map(|_| HeightUnit::Cm),
        tag([0x01].as_ref()).map(|_| HeightUnit::Inch),
    ))
    .parse(data)?;

    Ok(Message::HeightUnit(height_unit))
}

fn parse_physical_limits_message(data: &[u8]) -> NomResult<'_, Message> {
    let (data, max) = take(2usize)(data)?;
    let max = deserialize_height(max);
    let (_, min) = take(2usize)(data)?;
    let min = deserialize_height(min);

    Ok(Message::PhysicalLimits { min, max })
}

fn parse_limited_status_message(data: &[u8]) -> NomResult<'_, Message> {
    let (_, _status) = take(1usize)(data)?;

    Ok(Message::LimitedStatus)
}

impl Debug for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Height(height) => f
                .debug_tuple("Height")
                .field(&format_hex(HEIGHT_CODE))
                .field(height)
                .finish(),
            Message::PhysicalLimits { min, max } => f
                .debug_tuple("PhysicalLimits")
                .field(&format_hex(PHYSICAL_LIMITS_CODE))
                .field(min)
                .field(max)
                .finish(),
            Message::HeightUnit(unit) => f
                .debug_tuple("HeightUnit")
                .field(&format_hex(HEIGHT_UNIT_CODE))
                .field(unit)
                .finish(),
            Message::LimitedStatus => f
                .debug_tuple("LimitedStatus")
                .field(&format_hex(LIMITED_STATUS_CODE))
                .finish(),
            Message::Unknown { code, data } => f
                .debug_struct("Unknown")
                .field("code", &format_hex(code))
                .field("data", &format_hex(data))
                .finish(),
        }
    }
}

#[inline]
fn format_hex<V: Debug>(value: V) -> String {
    format!("{value:02x?}")
}

#[cfg(test)]
mod tests {
    pub use super::*;
    use crate::api::serialize_payload;

    #[test]
    fn test_height_message() {
        let data = [0xf2, 0xf2, HEIGHT_CODE, 0x03, 0x01, 0x17, 0x03, 0x1f, 0x7e];
        let (i, result) = Message::parse(&data).unwrap();
        assert_eq!(i.len(), 0);
        assert_eq!(result, vec![Message::Height(0x0117)]);
    }

    #[test]
    fn test_height_unit_message() {
        let data = [
            0xf2,
            0xf2,
            HEIGHT_UNIT_CODE,
            0x01,
            0x00,
            HEIGHT_UNIT_CODE.wrapping_add(0x01),
            0x7e,
        ];
        let (i, result) = Message::parse(&data).unwrap();
        assert_eq!(i.len(), 0);
        assert_eq!(result, vec![Message::HeightUnit(HeightUnit::Cm)]);
        let data = [
            0xf2,
            0xf2,
            HEIGHT_UNIT_CODE,
            0x01,
            0x01,
            HEIGHT_UNIT_CODE.wrapping_add(0x02),
            0x7e,
        ];
        let (i, result) = Message::parse(&data).unwrap();
        assert_eq!(i.len(), 0);
        assert_eq!(result, vec![Message::HeightUnit(HeightUnit::Inch)]);
    }

    #[test]
    fn test_physical_limits_message() {
        let data = serialize_payload(&MESSAGE_PREFIX, 0x07, &[0x04, 0x56, 0x01, 0x23]);
        let (i, result) = Message::parse(&data).unwrap();
        assert_eq!(i.len(), 0);
        assert_eq!(
            result,
            vec![Message::PhysicalLimits {
                min: 0x0123,
                max: 0x0456
            }]
        );
    }

    #[test]
    fn test_parse_many() {
        let data = [
            serialize_payload(
                &MESSAGE_PREFIX,
                PHYSICAL_LIMITS_CODE,
                &[0x04, 0x56, 0x01, 0x23],
            ),
            serialize_payload(&MESSAGE_PREFIX, HEIGHT_CODE, &[0x01, 0x17, 0x03]),
            serialize_payload(
                &MESSAGE_PREFIX,
                PHYSICAL_LIMITS_CODE,
                &[0x04, 0x56, 0x01, 0x23],
            ),
        ]
        .concat();
        let (i, result) = Message::parse(&data).unwrap();

        assert_eq!(i.len(), 0);
        assert_eq!(
            result,
            vec![
                Message::PhysicalLimits {
                    min: 0x0123,
                    max: 0x0456
                },
                Message::Height(0x0117),
                Message::PhysicalLimits {
                    min: 0x0123,
                    max: 0x0456
                }
            ]
        );
    }

    #[test]
    fn test_parse_partial() {
        let mut data = serialize_payload(
            &MESSAGE_PREFIX,
            PHYSICAL_LIMITS_CODE,
            &[0x04, 0x56, 0x01, 0x23],
        );
        data.pop();
        let (i, result) = Message::parse(&data).unwrap();
        assert_eq!(i.len(), data.len());
        assert_eq!(result, vec![]);
        let first_message = serialize_payload(
            &MESSAGE_PREFIX,
            PHYSICAL_LIMITS_CODE,
            &[0x04, 0x56, 0x01, 0x23],
        );
        let mut data = [
            first_message.clone(),
            serialize_payload(&MESSAGE_PREFIX, HEIGHT_CODE, &[0x01, 0x17, 0x03]),
        ]
        .concat();
        data.pop();
        let (i, result) = Message::parse(&data).unwrap();

        assert_eq!(i.len(), data.len() - first_message.len());
        assert_eq!(
            result,
            vec![Message::PhysicalLimits {
                min: 0x0123,
                max: 0x0456
            }]
        );
    }
}
