//! `/dev/rfkill` (CC8): airplane mode soft-blocks every radio, as GNOME does, and the tile
//! follows the events the kernel queues. This file is the pure part: the 8-byte
//! `struct rfkill_event` the kernel reads and writes, and the state a stream of them adds up
//! to. The device itself is opened in `ui`.

use std::collections::BTreeMap;

/// `RFKILL_TYPE_ALL`.
pub const TYPE_ALL: u8 = 0;
/// `RFKILL_OP_ADD`, `RFKILL_OP_DEL`, `RFKILL_OP_CHANGE`: a radio appeared, left, or changed.
pub const OP_ADD: u8 = 0;
pub const OP_DEL: u8 = 1;
pub const OP_CHANGE: u8 = 2;
/// `RFKILL_OP_CHANGE_ALL`: sets the soft block of every radio of a type.
pub const OP_CHANGE_ALL: u8 = 3;
/// `RFKILL_EVENT_SIZE_V1`: `idx` (u32), `type`, `op`, `soft` and `hard` (u8 each). The kernel
/// accepts and returns this size on a read or write of that length.
pub const EVENT_SIZE: usize = 8;

/// A `struct rfkill_event`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub idx: u32,
    pub kind: u8,
    pub op: u8,
    pub soft: bool,
    pub hard: bool,
}

impl Event {
    /// The bytes of the struct as this host lays it out: the kernel copies it as it is, so
    /// `idx` is in the host's byte order.
    pub fn encode(self) -> [u8; EVENT_SIZE] {
        let idx = self.idx.to_ne_bytes();
        [
            idx[0],
            idx[1],
            idx[2],
            idx[3],
            self.kind,
            self.op,
            u8::from(self.soft),
            u8::from(self.hard),
        ]
    }

    pub fn decode(bytes: [u8; EVENT_SIZE]) -> Event {
        Event {
            idx: u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            kind: bytes[4],
            op: bytes[5],
            soft: bytes[6] != 0,
            hard: bytes[7] != 0,
        }
    }
}

/// The command that sets the soft block of every radio at once.
pub fn block_all(blocked: bool) -> Event {
    Event {
        idx: 0,
        kind: TYPE_ALL,
        op: OP_CHANGE_ALL,
        soft: blocked,
        hard: false,
    }
}

/// The radios the kernel has told us of: soft and hard block of each, by index.
#[derive(Debug, Default)]
pub struct Radios(BTreeMap<u32, (bool, bool)>);

impl Radios {
    pub fn apply(&mut self, event: Event) {
        match event.op {
            OP_ADD | OP_CHANGE => {
                self.0.insert(event.idx, (event.soft, event.hard));
            }
            OP_DEL => {
                self.0.remove(&event.idx);
            }
            _ => {}
        }
    }

    pub fn count(&self) -> usize {
        self.0.len()
    }

    /// Airplane mode is on when there is a radio and none of them can transmit.
    pub fn airplane(&self) -> bool {
        !self.0.is_empty() && self.0.values().all(|(soft, hard)| *soft || *hard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_the_eight_bytes_the_kernel_reads() {
        let bytes = block_all(true).encode();
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes[..4], 0u32.to_ne_bytes());
        assert_eq!(bytes[4..], [0, 3, 1, 0], "type ALL, op CHANGE_ALL, soft, not hard");
        assert_eq!(block_all(false).encode()[6], 0);
    }

    #[test]
    fn idx_keeps_the_hosts_byte_order() {
        let event = Event {
            idx: 0x0102_0304,
            kind: 2,
            op: OP_CHANGE,
            soft: false,
            hard: true,
        };
        let bytes = event.encode();
        assert_eq!(bytes[..4], 0x0102_0304u32.to_ne_bytes());
        assert_eq!(bytes[4..], [2, 2, 0, 1]);
        assert_eq!(Event::decode(bytes), event);
    }

    fn event(idx: u32, op: u8, soft: bool, hard: bool) -> Event {
        Event {
            idx,
            kind: 1,
            op,
            soft,
            hard,
        }
    }

    #[test]
    fn airplane_mode_is_every_radio_blocked_and_at_least_one_radio() {
        let mut radios = Radios::default();
        assert!(!radios.airplane(), "no radio, no airplane mode");
        radios.apply(event(0, OP_ADD, true, false));
        radios.apply(event(1, OP_ADD, false, false));
        assert!(!radios.airplane(), "one radio is still on");
        radios.apply(event(1, OP_CHANGE, true, false));
        assert!(radios.airplane());
        radios.apply(event(2, OP_ADD, false, true));
        assert!(radios.airplane(), "a hard block cannot transmit either");
        radios.apply(event(2, OP_CHANGE, false, false));
        assert!(!radios.airplane());
        radios.apply(event(2, OP_DEL, false, false));
        assert!(radios.airplane(), "a radio that left no longer counts");
    }

    #[test]
    fn a_command_is_not_a_radio() {
        let mut radios = Radios::default();
        radios.apply(block_all(true));
        assert!(!radios.airplane());
    }
}
