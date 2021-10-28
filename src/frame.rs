use std::{
    convert::TryFrom,
    fmt::Display,
    io::{self, Read},
    vec,
};

use crate::message::Message;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpCode {
    Continuation,
    Text,
    Binary,
    ConnectionClose,
    Ping,
    Pong,
    NonControl(u8),
    Control(u8),
}

#[derive(Debug)]
pub enum FrameError {
    CantConvertToMessage,
    InvalidOpCode,
    WouldBlock,
    Eof,
}
impl Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Can't convert frame to message")
    }
}
impl std::error::Error for FrameError {}

#[derive(Debug)]
pub struct Frame {
    pub fin: bool,
    pub rsv1: bool,
    pub rsv2: bool,
    pub rsv3: bool,
    pub opcode: OpCode,
    pub mask: bool,
    pub masking_key: Option<[u8; 4]>,
    pub extension_data: Vec<u8>,
    pub application_data: Vec<u8>,
}

impl Frame {
    pub fn from_fragmented(frames: &[Self]) -> Self {
        let application_data: Vec<u8> = frames
            .iter()
            .map(|frame| &frame.application_data)
            .flatten()
            .cloned()
            .collect();

        let first_frame = &frames[0];

        Frame {
            opcode: first_frame.opcode,
            fin: true,
            application_data,
            ..Default::default()
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = vec![];

        let mut b = ((self.fin as u8) << 7)
            | ((self.rsv1 as u8) << 6)
            | ((self.rsv2 as u8) << 5)
            | ((self.rsv3 as u8) << 4);

        b |= match self.opcode {
            OpCode::Continuation => 0x0,
            OpCode::Text => 0x1,
            OpCode::Binary => 0x2,
            OpCode::ConnectionClose => 0x8,
            OpCode::Ping => 0x9,
            OpCode::Pong => 0xA,
            OpCode::NonControl(code) => {
                assert!(code <= 4);
                0x3 + code
            }
            OpCode::Control(code) => {
                assert!(code <= 4);
                0xB + code
            }
        };

        bytes.push(b);

        b = (self.mask as u8) << 7;

        let total_len = self.application_data.len();
        if total_len <= 125 {
            b |= (total_len as u8).to_be();
        } else if (total_len as u16) <= u16::MAX {
            b |= (126_u8).to_be();
        } else {
            b |= (127_u8).to_be();
        }

        bytes.push(b);

        if total_len > 125 {
            bytes.extend_from_slice(&total_len.to_be_bytes());
        }

        if let Some(key) = self.masking_key {
            bytes.extend_from_slice(&key);
            bytes.extend_from_slice(&Self::decode_or_encode_masked_data(
                &key,
                &self.application_data,
            ));
        } else {
            bytes.extend_from_slice(&self.application_data);
        }

        bytes
    }

    fn take_bytes<R, const M: usize>(r: &mut R) -> Result<[u8; M], FrameError>
    where
        R: Read,
    {
        let mut buf = [0; M];
        r.read_exact(&mut buf)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::WouldBlock {
                    FrameError::WouldBlock
                } else {
                    FrameError::Eof
                }
            })
            .and(Ok(buf))
    }

    fn decode_or_encode_masked_data(masking_key: &[u8; 4], data: &[u8]) -> Vec<u8> {
        data.iter()
            .enumerate()
            .map(|(index, u)| u ^ masking_key[index % 4])
            .collect()
    }

    pub fn read<R: Read>(r: &mut R) -> Result<Self, FrameError> {
        let first_two_bytes = Self::take_bytes::<_, 2>(r)?;

        let first_byte = first_two_bytes[0];
        let fin = (first_byte >> 7) == 1;
        let rsv1 = ((first_byte >> 6) & 1) == 1;
        let rsv2 = ((first_byte >> 5) & 1) == 1;
        let rsv3 = ((first_byte >> 4) & 1) == 1;
        let opcode = {
            let b = first_byte & 0xF;
            match b {
                0x0 => OpCode::Continuation,
                0x1 => OpCode::Text,
                0x2 => OpCode::Binary,
                0x8 => OpCode::ConnectionClose,
                0x9 => OpCode::Ping,
                0xA => OpCode::Pong,
                0xB..=0xF => OpCode::Control(b - 0xB),
                0x3..=0x7 => OpCode::NonControl(b - 0x3),
                _ => return Err(FrameError::InvalidOpCode),
            }
        };
        let mask_and_payload_len = first_two_bytes[1];
        let mask = (mask_and_payload_len >> 7) == 1;
        let payload_len: u64 = {
            let x = mask_and_payload_len & 0x7f;
            match x {
                0..=125 => x.into(),
                126 => {
                    let buf = Self::take_bytes::<_, 2>(r)?;
                    u16::from_be_bytes(buf).into()
                }
                127..=255 => {
                    let buf = Self::take_bytes::<_, 8>(r)?;
                    u64::from_be_bytes(buf)
                }
            }
        };
        let masking_key: Option<[u8; 4]> = {
            if mask {
                let buf = Self::take_bytes::<_, 4>(r)?;
                Some(buf)
            } else {
                None
            }
        };
        let application_data: Vec<u8> = {
            let mut raw_payload_data: Vec<u8> = vec![0; payload_len as usize];
            r.read_exact(&mut raw_payload_data)
                .map_err(|_e| FrameError::Eof)?;

            match masking_key {
                Some(key) => Self::decode_or_encode_masked_data(&key, &raw_payload_data),
                None => raw_payload_data.to_vec(),
            }
        };

        Ok(Self {
            fin,
            rsv1,
            rsv2,
            rsv3,
            application_data,
            extension_data: vec![],
            masking_key,
            mask,
            opcode,
        })
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            application_data: vec![],
            fin: true,
            extension_data: vec![],
            mask: false,
            masking_key: None,
            opcode: OpCode::Binary,
            rsv1: false,
            rsv2: false,
            rsv3: false,
        }
    }
}

impl TryFrom<Frame> for Message {
    type Error = FrameError;
    fn try_from(mut f: Frame) -> Result<Self, Self::Error> {
        match f.opcode {
            OpCode::Binary => Ok(Message::Binary(std::mem::take(&mut f.application_data))),
            OpCode::Text => {
                let s = String::from_utf8(std::mem::take(&mut f.application_data))
                    .map_err(|_e| Self::Error::CantConvertToMessage)?;
                Ok(Message::Text(s))
            }
            _ => Err(Self::Error::CantConvertToMessage),
        }
    }
}

impl From<Message> for Frame {
    fn from(m: Message) -> Self {
        let (opcode, application_data) = match m {
            Message::Binary(b) => (OpCode::Binary, b),
            Message::Ping => (OpCode::Ping, vec![]),
            Message::Pong => (OpCode::Pong, vec![]),
            Message::Text(t) => (OpCode::Text, t.as_bytes().to_vec()),
        };

        Frame {
            opcode,
            mask: false,
            masking_key: None,
            application_data,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::frame::OpCode;

    use super::Frame;

    #[test]
    fn can_serialize_frames() {
        let frame = Frame {
            application_data: "hello".as_bytes().to_vec(),
            opcode: OpCode::Text,
            ..Default::default()
        };

        let frame_bytes = frame.to_bytes();
        let mut slice = frame_bytes.as_slice();

        let read_frame = Frame::read(&mut slice).unwrap();

        assert_eq!(read_frame.application_data, frame.application_data);
        assert_eq!(read_frame.fin, frame.fin);
        assert_eq!(read_frame.mask, frame.mask);
        assert_eq!(read_frame.opcode, frame.opcode);
    }
}
