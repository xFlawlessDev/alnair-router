#[derive(Debug, Default)]
pub(crate) struct SseLineBuffer {
    buffer: String,
}

impl SseLineBuffer {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));

        let mut lines = Vec::new();
        while let Some(newline_index) = self.buffer.find('\n') {
            let mut line: String = self.buffer.drain(..=newline_index).collect();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            lines.push(line);
        }

        lines
    }

    pub(crate) fn finish(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }
        let mut line = std::mem::take(&mut self.buffer);
        if line.ends_with('\r') {
            line.pop();
        }
        Some(line)
    }
}

pub(crate) fn parse_data_line(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return None;
    }
    let data = line.strip_prefix("data:")?;
    Some(data.trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_data_line_accepts_space_after_colon() {
        assert_eq!(parse_data_line(r#"data: {"x":1}"#), Some(r#"{"x":1}"#));
    }

    #[test]
    fn parse_data_line_accepts_no_space_after_colon() {
        assert_eq!(parse_data_line(r#"data:{"x":1}"#), Some(r#"{"x":1}"#));
    }

    #[test]
    fn finish_returns_leftover_final_line() {
        let mut buffer = SseLineBuffer::default();
        assert!(buffer.push(b"data: {\"x\":1}").is_empty());
        assert_eq!(buffer.finish(), Some(r#"data: {"x":1}"#.to_string()));
    }

    #[test]
    fn push_preserves_done_payload() {
        let mut buffer = SseLineBuffer::default();
        let lines = buffer.push(b"data: [DONE]\n");
        assert_eq!(parse_data_line(&lines[0]), Some("[DONE]"));
    }
}
