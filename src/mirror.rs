//! Ordered read-only ANSI screen reconstruction with explicit stream failures.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::io::{self, BufRead, Read};
use thiserror::Error;

pub const LINE_LIMIT: usize = 8 * 1024 * 1024;
const MAX_CELLS: usize = 2 * 1024 * 1024;

/// Read one complete bounded NDJSON record. Silence is not an EOF.
pub fn read_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let count = reader
        .take((LINE_LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    if count > LINE_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "observer record exceeds 8 MiB",
        ));
    }
    if bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "observer closed during an incomplete record",
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("invalid observer record: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid base64 frame: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("frame sequence gap: expected {expected}, received {actual}")]
    Sequence { expected: u64, actual: u64 },
    #[error("observer did not begin with a full frame")]
    MissingFull,
    #[error("unsupported observer encoding: {0}")]
    Encoding(String),
    #[error("invalid observer screen dimensions: {width} x {height}")]
    Dimensions { width: u16, height: u16 },
    #[error("observer changed dimensions without a full frame")]
    DeltaResize,
    #[error("observer frame sequence exhausted")]
    SequenceOverflow,
    #[error("observer record exceeded 8 MiB")]
    Oversize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum StreamEvent {
    Applied,
    Closed(Option<String>),
    Ignored,
}

#[derive(Deserialize)]
pub struct Frame {
    pub seq: u64,
    pub encoding: String,
    pub width: u16,
    pub height: u16,
    pub full: bool,
    pub bytes: String,
}

pub struct TerminalMirror {
    parser: vt100::Parser,
    next_seq: u64,
    initialized: bool,
}
impl TerminalMirror {
    pub fn new(rows: u16, cols: u16) -> Self {
        // The viewer renders a screen, not terminal scrollback; bound parser memory.
        Self {
            parser: vt100::Parser::new(rows.max(1), cols.max(1), 0),
            next_seq: 1,
            initialized: false,
        }
    }
    pub fn apply_json(&mut self, line: &str) -> Result<StreamEvent, StreamError> {
        if line.len() > LINE_LIMIT {
            return Err(StreamError::Oversize);
        }
        let value: serde_json::Value = serde_json::from_str(line)?;
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("terminal.closed") => {
                return Ok(StreamEvent::Closed(
                    value
                        .get("reason")
                        .or_else(|| value.get("message"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                ));
            }
            Some("terminal.frame") => {}
            _ => return Ok(StreamEvent::Ignored),
        }
        let f: Frame = serde_json::from_value(value)?;
        if f.encoding != "ansi" {
            return Err(StreamError::Encoding(f.encoding));
        }
        if f.width == 0 || f.height == 0 || usize::from(f.width) * usize::from(f.height) > MAX_CELLS
        {
            return Err(StreamError::Dimensions {
                width: f.width,
                height: f.height,
            });
        }
        if f.seq != self.next_seq {
            return Err(StreamError::Sequence {
                expected: self.next_seq,
                actual: f.seq,
            });
        }
        if !self.initialized && !f.full {
            return Err(StreamError::MissingFull);
        }
        if self.initialized && !f.full && self.parser.screen().size() != (f.height, f.width) {
            return Err(StreamError::DeltaResize);
        }
        let next_seq = f.seq.checked_add(1).ok_or(StreamError::SequenceOverflow)?;
        let bytes = STANDARD.decode(f.bytes)?;
        // Validate before altering the prior screen so a failure retains useful output.
        if f.full {
            self.parser = vt100::Parser::new(f.height, f.width, 0);
        }
        self.parser.process(&bytes);
        self.initialized = true;
        self.next_seq = next_seq;
        Ok(StreamEvent::Applied)
    }
    pub fn initialized(&self) -> bool {
        self.initialized
    }
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(seq: u64, full: bool, text: &[u8]) -> String {
        serde_json::json!({"type":"terminal.frame","seq":seq,"encoding":"ansi","width":8,"height":2,"full":full,"bytes":STANDARD.encode(text)}).to_string()
    }
    #[test]
    fn applies_full_and_ordered_cursor_delta_frames() {
        let mut m = TerminalMirror::new(2, 8);
        assert_eq!(
            m.apply_json(&frame(1, true, b"Hi")).unwrap(),
            StreamEvent::Applied
        );
        m.apply_json(&frame(2, false, b"\x1b[1;1Hthere")).unwrap();
        assert!(m.initialized());
        assert_eq!(m.screen().contents().trim(), "there");
    }
    #[test]
    fn gap_or_invalid_base64_preserves_prior_screen_and_sequence() {
        let mut m = TerminalMirror::new(2, 8);
        m.apply_json(&frame(1, true, b"Hi")).unwrap();
        assert!(matches!(
            m.apply_json(&frame(3, false, b"lost")),
            Err(StreamError::Sequence { .. })
        ));
        let bad = frame(2, false, b"next").replace(&STANDARD.encode(b"next"), "???");
        assert!(matches!(m.apply_json(&bad), Err(StreamError::Base64(_))));
        assert_eq!(m.screen().contents().trim(), "Hi");
        m.apply_json(&frame(2, false, b"!")).unwrap();
        assert_eq!(m.screen().contents().trim(), "Hi!");
    }
    #[test]
    fn missing_baseline_unsupported_encoding_and_delta_resize_are_errors() {
        let mut m = TerminalMirror::new(2, 8);
        assert!(matches!(
            m.apply_json(&frame(1, false, b"oops")),
            Err(StreamError::MissingFull)
        ));
        assert!(matches!(
            m.apply_json(&frame(1, true, b"x").replace("ansi", "sixel")),
            Err(StreamError::Encoding(_))
        ));
        m.apply_json(&frame(1, true, b"Hi")).unwrap();
        assert!(matches!(
            m.apply_json(&frame(2, false, b"x").replace("\"height\":2", "\"height\":3")),
            Err(StreamError::DeltaResize)
        ));
    }
    #[test]
    fn closure_requires_no_frame_fields_and_full_frame_can_resize() {
        let mut m = TerminalMirror::new(2, 8);
        m.apply_json(&frame(1, true, b"Hi")).unwrap();
        assert_eq!(
            m.apply_json(r#"{"type":"terminal.closed","reason":"source ended"}"#)
                .unwrap(),
            StreamEvent::Closed(Some("source ended".into()))
        );
        m.apply_json(&frame(2, true, b"New").replace("\"height\":2", "\"height\":3"))
            .unwrap();
        assert_eq!(m.screen().size(), (3, 8));
        assert_eq!(m.screen().contents().trim(), "New");
    }
    #[test]
    fn bounded_reader_distinguishes_eof_partial_records_and_oversize() {
        assert!(
            read_line(&mut io::Cursor::new(b"" as &[u8]))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            read_line(&mut io::Cursor::new(b"{}\n")).unwrap().unwrap(),
            "{}\n"
        );
        assert_eq!(
            read_line(&mut io::Cursor::new(b"{}")).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            read_line(&mut io::Cursor::new(vec![b'x'; LINE_LIMIT + 1]))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
