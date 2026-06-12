//! Incremental framing of multi-line JSON objects from the CLI's stdout
//!
//! The Filen CLI pretty-prints JSON events across multiple lines. This module
//! accumulates lines until a complete top-level JSON object has been seen.
//!
//! Unlike naive brace counting, the framer is aware of JSON string literals
//! and escape sequences, so braces inside file names (e.g. `file{1.txt`)
//! cannot corrupt the depth tracking. It also recovers from stray closing
//! braces in plain-text output and caps the buffer size so a misbehaving
//! CLI cannot grow memory unboundedly.

/// Maximum bytes buffered while waiting for a JSON object to complete.
/// Anything larger is not a sync event we care about; discard it.
const MAX_BUFFER_BYTES: usize = 1024 * 1024;

/// A complete unit of CLI output: either a JSON object or a plain text line.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    Json(String),
    Text(String),
}

/// Accumulates CLI stdout lines into complete JSON objects or text lines.
#[derive(Default)]
pub struct JsonFramer {
    buffer: String,
    depth: i32,
    in_string: bool,
    escaped: bool,
}

impl JsonFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one line of CLI output. Returns any frames completed by this line.
    pub fn push_line(&mut self, line: &str) -> Vec<Frame> {
        let mut frames = Vec::new();

        // Plain text line while not inside a JSON object
        if self.buffer.is_empty() && !line.trim_start().starts_with('{') {
            let text = line.trim();
            if !text.is_empty() {
                frames.push(Frame::Text(text.to_string()));
            }
            return frames;
        }

        for ch in line.chars() {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if ch == '\\' {
                    self.escaped = true;
                } else if ch == '"' {
                    self.in_string = false;
                }
                continue;
            }
            match ch {
                '"' => self.in_string = true,
                '{' => self.depth += 1,
                '}' => self.depth -= 1,
                _ => {}
            }
        }

        self.buffer.push_str(line);
        self.buffer.push('\n');

        if self.depth <= 0 {
            let complete = self.buffer.trim();
            if !complete.is_empty() {
                if complete.starts_with('{') && self.depth == 0 {
                    frames.push(Frame::Json(complete.to_string()));
                } else {
                    // Stray closing braces or malformed output: surface as text
                    frames.push(Frame::Text(complete.to_string()));
                }
            }
            self.reset();
        } else if self.buffer.len() > MAX_BUFFER_BYTES {
            log::warn!(
                "Discarding oversized CLI output buffer ({} bytes)",
                self.buffer.len()
            );
            self.reset();
        }

        frames
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.depth = 0;
        self.in_string = false;
        self.escaped = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(s: &str) -> Frame {
        Frame::Json(s.to_string())
    }

    #[test]
    fn test_single_line_json() {
        let mut f = JsonFramer::new();
        let frames = f.push_line(r#"{"type":"cycleSuccess"}"#);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
    }

    #[test]
    fn test_multi_line_json() {
        let mut f = JsonFramer::new();
        assert!(f.push_line("{").is_empty());
        assert!(f.push_line(r#"  "type": "cycleSuccess""#).is_empty());
        let frames = f.push_line("}");
        assert_eq!(frames.len(), 1);
        match &frames[0] {
            Frame::Json(s) => {
                let parsed: serde_json::Value = serde_json::from_str(s).unwrap();
                assert_eq!(parsed["type"], "cycleSuccess");
            }
            other => panic!("Expected Json frame, got {:?}", other),
        }
    }

    #[test]
    fn test_braces_inside_strings_do_not_break_framing() {
        let mut f = JsonFramer::new();
        // Unbalanced brace inside a string value (e.g. a weird file name)
        let line = r#"{"type":"transfer","data":{"relativePath":"dir/file{1.txt"}}"#;
        let frames = f.push_line(line);
        assert_eq!(frames, vec![json(line)]);
        // Framer must be clean for the next event
        let frames = f.push_line(r#"{"type":"cycleSuccess"}"#);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
    }

    #[test]
    fn test_escaped_quotes_inside_strings() {
        let mut f = JsonFramer::new();
        let line = r#"{"path":"file \"quoted{\" name"}"#;
        let frames = f.push_line(line);
        assert_eq!(frames, vec![json(line)]);
    }

    #[test]
    fn test_plain_text_passthrough() {
        let mut f = JsonFramer::new();
        let frames = f.push_line("Done syncing.");
        assert_eq!(frames, vec![Frame::Text("Done syncing.".to_string())]);
    }

    #[test]
    fn test_stray_closing_brace_recovers() {
        let mut f = JsonFramer::new();
        let frames = f.push_line("} unexpected");
        // Surfaced as text, framer resets cleanly
        assert_eq!(frames, vec![Frame::Text("} unexpected".to_string())]);
        let frames = f.push_line(r#"{"type":"cycleSuccess"}"#);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
    }

    #[test]
    fn test_empty_lines_ignored() {
        let mut f = JsonFramer::new();
        assert!(f.push_line("").is_empty());
        assert!(f.push_line("   ").is_empty());
    }

    #[test]
    fn test_oversized_buffer_discarded() {
        let mut f = JsonFramer::new();
        f.push_line("{");
        let big_line = format!("\"key\": \"{}\",", "x".repeat(600 * 1024));
        f.push_line(&big_line);
        f.push_line(&big_line);
        // Buffer exceeded the cap and was discarded; framer accepts new events
        let frames = f.push_line(r#"{"type":"cycleSuccess"}"#);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
    }

    #[test]
    fn test_interleaved_text_between_json() {
        let mut f = JsonFramer::new();
        assert_eq!(
            f.push_line("Syncing 3 files"),
            vec![Frame::Text("Syncing 3 files".to_string())]
        );
        assert_eq!(
            f.push_line(r#"{"type":"cycleSuccess"}"#),
            vec![json(r#"{"type":"cycleSuccess"}"#)]
        );
    }
}
