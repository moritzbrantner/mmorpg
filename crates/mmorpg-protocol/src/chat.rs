//! Chat lines on the wire: speaker (`u32`), channel (`u8`: 0 say, 1 yell),
//! text length (`u8`, 1–80) and the UTF-8 text, validated like core.

use mmorpg_core::{ChatChannel, ChatLine, ChatText, MAX_CHAT_BYTES};

use crate::ProtocolError;
use crate::wire::{read_u8, take};

/// The largest chat record: speaker, channel, length and 80 text bytes.
pub const MAX_CHAT_RECORD_BYTES: usize = 4 + 1 + 1 + MAX_CHAT_BYTES;

/// Channel, length and text, as a command and a record carry them.
pub(crate) fn encode_text(payload: &mut Vec<u8>, channel: ChatChannel, text: &ChatText) {
    payload.push(channel.code());
    let bytes = text.as_str().as_bytes();
    // ChatText holds 1–80 bytes, so the length fits a u8.
    payload.push(u8::try_from(bytes.len()).unwrap_or(0));
    payload.extend_from_slice(bytes);
}

pub(crate) fn decode_text(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(ChatChannel, ChatText), ProtocolError> {
    let channel = ChatChannel::from_code(read_u8(payload, offset)?)
        .ok_or_else(|| ProtocolError::new("unknown chat channel"))?;
    let len = usize::from(read_u8(payload, offset)?);
    if len == 0 || len > MAX_CHAT_BYTES {
        return Err(ProtocolError::new("chat text must be 1 to 80 bytes"));
    }
    let end = offset
        .checked_add(len)
        .filter(|&end| end <= payload.len())
        .ok_or_else(|| ProtocolError::new("chat text is truncated"))?;
    let text = ChatText::from_utf8(&payload[*offset..end])
        .map_err(|error| ProtocolError::new(error.message().to_owned()))?;
    *offset = end;
    Ok((channel, text))
}

pub(crate) fn encode_line(payload: &mut Vec<u8>, line: &ChatLine) {
    payload.extend_from_slice(&line.speaker.to_be_bytes());
    encode_text(payload, line.channel, &line.text);
}

pub(crate) fn decode_line(payload: &[u8], offset: &mut usize) -> Result<ChatLine, ProtocolError> {
    let speaker = u32::from_be_bytes(take(payload, offset)?);
    let (channel, text) = decode_text(payload, offset)?;
    Ok(ChatLine {
        speaker,
        channel,
        text,
    })
}
