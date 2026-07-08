use crate::api::{serialize_height, serialize_payload};
use btleplug::Result;
use btleplug::api;
use btleplug::api::{Characteristic, WriteType};
use std::borrow::Cow;

/// https://gitlab.com/pimp-my-desk/desk-control/jiecang-reverse-engineering#protocol
/// https://github.com/phord/Jarvis#hsx-control-lines
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub enum Command {
    Up,
    Down,
    Stop,
    SaveSit,
    SaveStand,
    SaveMemory1,
    SaveMemory2,
    SaveMemory3,
    SaveMemory4,
    GotoSit,
    GotoStand,
    GotoMemory1,
    GotoMemory2,
    GotoMemory3,
    GotoMemory4,
    FetchHeightValue,
    FetchHeightRange,
    GotoHeight { height_mm: u16 },
    LockInfo,
    SetMemoryClickMode { during_move: bool },
    FetchHighLowLimit,
    SaveUpLimit,
    SaveDownLimit,
    ClearLimit { direction: Direction },
    Patch,
    FetchStandTime,
    FetchAllTime,
    SetMemoryHeight { memory_location: u8, height_mm: u16 },
    HandOperate,
    Unknown { cmd: u8, data: &'static [u8] },
}

#[derive(Copy, Clone, Debug)]
pub enum Direction {
    All,
    Up,
    Down,
}

pub const UP: [u8; 6] = simple_desk_command(0x01);
pub const DOWN: [u8; 6] = simple_desk_command(0x02);
/// Save Sit
pub const SAVE_MEMORY_1: [u8; 6] = simple_desk_command(0x03);
/// Save Stand
pub const SAVE_MEMORY_2: [u8; 6] = simple_desk_command(0x04);
/// Goto Sit
pub const GOTO_MEMORY_1: [u8; 6] = simple_desk_command(0x05);
/// Goto Stand
pub const GOTO_MEMORY_2: [u8; 6] = simple_desk_command(0x06);
pub const FETCH_HEIGHT_VALUE: [u8; 6] = simple_desk_command(0x07);
pub const FETCH_HEIGHT_RANGE: [u8; 6] = simple_desk_command(0x0C);

pub fn goto_height(height_mm: u16) -> Vec<u8> {
    advanced_desk_command(0x1b, &serialize_height(height_mm))
}

// Unknown what this does
pub fn lock_info() -> Vec<u8> {
    // I'm not sure what the data denotes here
    advanced_desk_command(0x1f, &[0x0])
}

pub fn set_memory_click_mode(mode: u8) -> Vec<u8> {
    advanced_desk_command(0x19, &[mode])
}

pub const FETCH_HIGH_LOW_LIMIT: [u8; 6] = simple_desk_command(0x20);

pub const SAVE_UP_LIMIT: [u8; 6] = simple_desk_command(0x21);
pub const SAVE_DOWN_LIMIT: [u8; 6] = simple_desk_command(0x22);

pub fn clear_limit(dir: Direction) -> Vec<u8> {
    advanced_desk_command(
        0x23,
        &[match dir {
            Direction::All => 0x0,
            Direction::Up => 0x1,
            Direction::Down => 0x2,
        }],
    )
}

pub const SAVE_MEMORY_3: [u8; 6] = simple_desk_command(0x25);
pub const SAVE_MEMORY_4: [u8; 6] = simple_desk_command(0x26);
pub const GOTO_MEMORY_3: [u8; 6] = simple_desk_command(0x27);
pub const GOTO_MEMORY_4: [u8; 6] = simple_desk_command(0x28);

pub const STOP: [u8; 6] = simple_desk_command(0x2b);

/// Unknown what this does
pub const PATCH: [u8; 6] = simple_desk_command(0xa0);

pub const FETCH_STAND_TIME: [u8; 6] = simple_desk_command(0xa2);
pub const FETCH_ALL_TIME: [u8; 6] = simple_desk_command(0xaa);

pub fn set_memory_height(memory_location: u8, height_mm: u16) -> Vec<u8> {
    assert!(memory_location > 0 && memory_location <= 3);
    let height_bytes = serialize_height(height_mm);
    advanced_desk_command(0xad, &[memory_location, height_bytes[0], height_bytes[1]])
}

pub const HAND_OPERATE: [u8; 6] = simple_desk_command(0xfe);

impl Command {
    pub(crate) fn payload(self) -> Cow<'static, [u8]> {
        match self {
            Command::Up => Cow::from(&UP),
            Command::Down => Cow::from(&DOWN),
            Command::Stop => Cow::from(&STOP),
            Command::SaveSit | Command::SaveMemory1 => Cow::from(&SAVE_MEMORY_1),
            Command::SaveStand | Command::SaveMemory2 => Cow::from(&SAVE_MEMORY_2),
            Command::SaveMemory3 => Cow::from(&SAVE_MEMORY_3),
            Command::SaveMemory4 => Cow::from(&SAVE_MEMORY_4),
            Command::GotoSit | Command::GotoMemory1 => Cow::from(&GOTO_MEMORY_1),
            Command::GotoStand | Command::GotoMemory2 => Cow::from(&GOTO_MEMORY_2),
            Command::GotoMemory3 => Cow::from(&GOTO_MEMORY_3),
            Command::GotoMemory4 => Cow::from(&GOTO_MEMORY_4),
            Command::FetchHeightValue => Cow::from(&FETCH_HEIGHT_VALUE),
            Command::FetchHeightRange => Cow::from(&FETCH_HEIGHT_RANGE),
            Command::GotoHeight { height_mm } => Cow::from(goto_height(height_mm)),
            Command::LockInfo => Cow::from(lock_info()),
            Command::SetMemoryClickMode { during_move } => {
                Cow::from(set_memory_click_mode(if during_move { 1 } else { 0 }))
            }
            Command::FetchHighLowLimit => Cow::from(&FETCH_HIGH_LOW_LIMIT),
            Command::SaveUpLimit => Cow::from(&SAVE_UP_LIMIT),
            Command::SaveDownLimit => Cow::from(&SAVE_DOWN_LIMIT),
            Command::ClearLimit { direction } => Cow::from(clear_limit(direction)),
            Command::Patch => Cow::from(&PATCH),
            Command::FetchStandTime => Cow::from(&FETCH_STAND_TIME),
            Command::FetchAllTime => Cow::from(&FETCH_ALL_TIME),
            Command::SetMemoryHeight {
                memory_location,
                height_mm,
            } => Cow::from(set_memory_height(memory_location, height_mm)),
            Command::HandOperate => Cow::from(&HAND_OPERATE),
            Command::Unknown { cmd, data } => Cow::from(advanced_desk_command(cmd, data)),
        }
    }
}

impl From<Command> for Cow<'static, [u8]> {
    fn from(val: Command) -> Self {
        val.payload()
    }
}

const CMD_PREFIX: [u8; 2] = [0xf1, 0xf1];

/// A simple command with no data
const fn simple_desk_command(cmd: u8) -> [u8; 6] {
    [0xf1, 0xf1, cmd, 0x00, cmd, 0x7e]
}

pub fn advanced_desk_command(cmd: u8, data: &[u8]) -> Vec<u8> {
    serialize_payload(&CMD_PREFIX, cmd, data)
}

pub(crate) trait EnhancedPeripheral {
    async fn command(
        &self,
        command: Command,
        characteristic: &Characteristic,
        write_type: WriteType,
    ) -> Result<()>;
}

impl<P: api::Peripheral> EnhancedPeripheral for P {
    async fn command(
        &self,
        command: Command,
        characteristic: &Characteristic,
        write_type: WriteType,
    ) -> Result<()> {
        let payload = command.payload();
        log::trace!(
            "{{{}}} - Sending Command: {command:?} @ {payload:02x?}",
            self.id()
        );
        self.write(characteristic, payload.as_ref(), write_type)
            .await
    }
}

#[cfg(test)]
mod tests {
    pub use super::*;

    #[test]
    fn test_commands() {
        // assert_eq!(UP, &[0xf1, 0xf1, 0x01, 0x00, 0x01, 0x7e]);
        assert_eq!(advanced_desk_command(0x01, &[]), simple_desk_command(0x01));
        assert_eq!(
            goto_height(0x0123),
            [
                0xf1,
                0xf1,
                0x1b,
                2,
                0x01,
                0x23,
                0x1b + 2 + 0x01 + 0x23,
                0x7e
            ]
        );
        assert_eq!(
            goto_height(0xfe12),
            [
                0xf1,
                0xf1,
                0x1b,
                2,
                0xfe,
                0x12,
                0x1bu8.wrapping_add(2).wrapping_add(0xfe).wrapping_add(0x12),
                0x7e
            ]
        );
        assert_eq!(
            set_memory_height(0x01, 0x0102),
            [
                0xf1,
                0xf1,
                0xad,
                3,
                0x01,
                0x01,
                0x02,
                0xad + 3 + 0x01 + 0x01 + 0x02,
                0x7e
            ]
        );
    }
}
