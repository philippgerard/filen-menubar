//! Incremental framing of multi-line JSON objects from the CLI's stdout
//!
//! The Filen CLI pretty-prints JSON events across multiple lines. This module
//! accumulates lines until a complete top-level JSON object has been seen.
//!
//! Unlike naive brace counting, the framer is aware of JSON string literals
//! and escape sequences, so braces inside file names (e.g. `file{1.txt`)
//! cannot corrupt the depth tracking. It also recovers from stray closing
//! braces in plain-text output and caps the buffer size so a misbehaving
//! CLI cannot grow memory unboundedly. Oversized objects are discarded while
//! their structure is still tracked, so nested fragments cannot be mistaken
//! for subsequent top-level events.

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
    discarding_text: bool,
    discarding: bool,
    discard_line_start: bool,
    discard_closing_line: bool,
}

impl JsonFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one chunk of CLI output. Returns every frame completed in it.
    ///
    /// Chunks may contain partial lines, many lines, or several JSON objects.
    /// Processing the stream directly is important because one ignored-tree
    /// event from the CLI can contain more than 100,000 pretty-printed lines.
    pub fn push_chunk(&mut self, chunk: &str) -> Vec<Frame> {
        let mut frames = Vec::new();

        for ch in chunk.chars() {
            // A diagnostic without a newline is not allowed to retain memory
            // forever. Once the top-level text limit is exceeded, discard the
            // rest of that record and resume framing after its newline.
            if self.discarding_text {
                if ch == '\n' {
                    self.discarding_text = false;
                }
                continue;
            }

            // At top level, collect ordinary CLI output until the next newline.
            // A top-level JSON object begins with `{`; tolerate indentation
            // before it without treating the whitespace as a text frame.
            if self.depth == 0 && !self.discarding {
                if ch == '{' && self.buffer.trim().is_empty() {
                    self.buffer.push(ch);
                    self.depth = 1;
                    continue;
                }

                if ch == '\n' {
                    let text = self.buffer.trim();
                    if !text.is_empty() {
                        frames.push(Frame::Text(text.to_string()));
                    }
                    self.buffer.clear();
                } else if ch != '\r' || !self.buffer.is_empty() {
                    self.buffer.push(ch);
                    if self.buffer.len() > MAX_BUFFER_BYTES {
                        log::warn!(
                            "Discarding oversized CLI text record after {} buffered bytes",
                            self.buffer.len()
                        );
                        self.buffer.clear();
                        self.discarding_text = true;
                    }
                }
                continue;
            }

            // The CLI pretty-prints each top-level event and places its outer
            // closing brace alone at column zero. Once an object is already
            // oversized and being discarded, this is a safe fallback boundary
            // even if malformed or truncated nested data left brace depth
            // inconsistent. Indented nested braces cannot match it.
            let discard_boundary = if self.discarding {
                match ch {
                    '\n' => {
                        let boundary = self.discard_closing_line;
                        self.discard_line_start = true;
                        self.discard_closing_line = false;
                        boundary
                    }
                    '\r' | ' ' | '\t' if self.discard_closing_line => false,
                    '}' if self.discard_line_start => {
                        self.discard_line_start = false;
                        self.discard_closing_line = true;
                        false
                    }
                    _ => {
                        self.discard_line_start = false;
                        self.discard_closing_line = false;
                        false
                    }
                }
            } else {
                false
            };

            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if ch == '\\' {
                    self.escaped = true;
                } else if ch == '"' {
                    self.in_string = false;
                }
            } else {
                match ch {
                    '"' => self.in_string = true,
                    '{' => self.depth += 1,
                    '}' => self.depth -= 1,
                    _ => {}
                }
            }

            if !self.discarding {
                self.buffer.push(ch);
            }

            if self.depth <= 0 || discard_boundary {
                if !self.discarding {
                    let complete = self.buffer.trim();
                    if !complete.is_empty() {
                        if complete.starts_with('{') && self.depth == 0 {
                            frames.push(Frame::Json(complete.to_string()));
                        } else {
                            frames.push(Frame::Text(complete.to_string()));
                        }
                    }
                }
                self.reset();
            } else if !self.discarding && self.buffer.len() > MAX_BUFFER_BYTES {
                log::warn!(
                    "Discarding oversized CLI JSON object after {} buffered bytes",
                    self.buffer.len()
                );
                self.buffer.clear();
                self.discarding = true;
                self.discard_line_start = ch == '\n';
                self.discard_closing_line = false;
            }
        }

        frames
    }

    /// Feed one line of CLI output. Retained for focused tests and callers that
    /// naturally receive complete lines.
    pub fn push_line(&mut self, line: &str) -> Vec<Frame> {
        let mut frames = self.push_chunk(line);
        frames.extend(self.push_chunk("\n"));
        frames
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.depth = 0;
        self.in_string = false;
        self.escaped = false;
        self.discarding_text = false;
        self.discarding = false;
        self.discard_line_start = false;
        self.discard_closing_line = false;
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
    fn test_chunk_can_contain_text_and_multiple_json_events() {
        let mut f = JsonFramer::new();
        let frames = f.push_chunk(
            "Filen CLI v0.0.39-menubar.2\n{\"type\":\"cycleStarted\"}\n\
             {\n  \"type\": \"cycleSuccess\"\n}\n",
        );

        assert_eq!(
            frames,
            vec![
                Frame::Text("Filen CLI v0.0.39-menubar.2".to_string()),
                json(r#"{"type":"cycleStarted"}"#),
                json("{\n  \"type\": \"cycleSuccess\"\n}"),
            ]
        );
    }

    #[test]
    fn test_chunk_boundaries_inside_strings_preserve_framing() {
        let mut f = JsonFramer::new();
        assert!(f.push_chunk(r#"{"path":"quoted \"#).is_empty());
        assert!(f.push_chunk(r#"" brace { and "#).is_empty());
        let frames = f.push_chunk(r#"text"}{"type":"cycleSuccess"}"#);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1], json(r#"{"type":"cycleSuccess"}"#));
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

        // The payload buffer is gone, but the framer must preserve the outer
        // object's structural state until its matching closing brace arrives.
        assert!(f.discarding);
        assert_eq!(f.depth, 1);

        // A complete-looking nested object is still part of the discarded
        // payload and must not be surfaced as a top-level event.
        assert!(f.push_line(r#"{"type":"cycleSuccess"}"#).is_empty());
        assert!(f.discarding);
        assert_eq!(f.depth, 1);

        // Closing the original object restores normal framing.
        assert!(f.push_line("}").is_empty());
        assert!(!f.discarding);
        assert_eq!(f.depth, 0);

        let frames = f.push_line(r#"{"type":"cycleSuccess"}"#);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
    }

    #[test]
    fn test_oversized_top_level_text_recovers_after_newline() {
        let mut f = JsonFramer::new();
        let oversized = "x".repeat(MAX_BUFFER_BYTES + 1);

        assert!(f.push_chunk(&oversized).is_empty());
        assert!(f.buffer.is_empty());
        assert!(f.discarding_text);

        let frames = f.push_chunk("\n{\"type\":\"cycleSuccess\"}\n");
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
        assert!(!f.discarding_text);
    }

    #[test]
    fn test_oversized_top_level_whitespace_is_bounded_and_recovers() {
        let mut f = JsonFramer::new();
        let oversized = " ".repeat(MAX_BUFFER_BYTES + 1);

        assert!(f.push_chunk(&oversized).is_empty());
        assert!(f.buffer.is_empty());

        let frames = f.push_chunk("\nhealthy diagnostic\n");
        assert_eq!(frames, vec![Frame::Text("healthy diagnostic".to_string())]);
    }

    #[test]
    fn test_oversized_pretty_printed_event_emits_no_fragments() {
        let mut f = JsonFramer::new();

        assert!(f.push_line("{").is_empty());
        assert!(f.push_line(r#"  "type": "remoteTreeIgnored","#).is_empty());
        assert!(f.push_line(r#"  "data": {"#).is_empty());
        assert!(f.push_line(r#"    "ignored": ["#).is_empty());

        let ignored_path = "x".repeat(2048);
        for index in 0..1024 {
            let frames = f.push_line(&format!(
                r#"      {{"relativePath":"/Photos/{index}/{ignored_path}","reason":"filenIgnore"}},"#
            ));
            assert!(
                frames.is_empty(),
                "oversized payload leaked a fragment frame at item {index}"
            );
        }

        assert!(f.discarding);
        assert!(f.push_line("    ]").is_empty());
        assert!(f.push_line("  }").is_empty());
        assert!(f.push_line("}").is_empty());
        assert!(!f.discarding);

        let frames = f.push_line(r#"{"type":"cycleSuccess"}"#);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
    }

    #[test]
    fn test_oversized_hundred_thousand_line_chunk_recovers_next_event() {
        let mut f = JsonFramer::new();
        let mut output = String::from("{\n  \"type\": \"remoteTreeIgnored\",\n  \"data\": [\n");
        output.push_str(&"    {\"path\":\"ignored\"},\n".repeat(110_000));
        output.push_str("    {\"path\":\"last\"}\n  ]\n}\n");
        output.push_str("{\"type\":\"cycleSuccess\"}\n");

        let frames = f.push_chunk(&output);
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
        assert!(!f.discarding);
        assert_eq!(f.depth, 0);
    }

    #[test]
    fn test_oversized_pretty_event_recovers_at_unindented_outer_close() {
        let mut f = JsonFramer::new();
        f.push_chunk("{\n  \"data\": {\n");
        f.push_chunk(&format!(
            "    \"payload\": \"{}\",\n",
            "x".repeat(1024 * 1024)
        ));
        assert!(f.discarding);

        // Simulate malformed/truncated nested data leaving structural depth
        // wrong. An indented nested close must not end the discarded frame.
        f.push_chunk("    {\n  }\n");
        assert!(f.discarding);

        // The CLI's column-zero outer close is an unambiguous frame boundary.
        let frames = f.push_chunk("}\n{\"type\":\"cycleSuccess\"}\n");
        assert_eq!(frames, vec![json(r#"{"type":"cycleSuccess"}"#)]);
        assert!(!f.discarding);
        assert_eq!(f.depth, 0);
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
