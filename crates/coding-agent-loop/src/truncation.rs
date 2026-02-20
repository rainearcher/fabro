use crate::config::SessionConfig;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationMode {
    HeadTail,
    Tail,
}

const TRUNCATION_WARNING_HEAD_TAIL: &str =
    "\n\n[WARNING: Output truncated. Showing first and last portions.]";
const TRUNCATION_WARNING_TAIL: &str =
    "\n\n[WARNING: Output truncated. Showing last portion only.]";
const LINE_TRUNCATION_WARNING: &str =
    "\n\n[WARNING: Output truncated by line count. Showing first and last lines.]";

fn default_char_limits() -> HashMap<&'static str, usize> {
    let mut m = HashMap::new();
    m.insert("read_file", 50_000);
    m.insert("shell", 30_000);
    m.insert("grep", 20_000);
    m.insert("glob", 20_000);
    m.insert("edit_file", 10_000);
    m.insert("write_file", 1_000);
    m
}

fn default_line_limits() -> HashMap<&'static str, usize> {
    let mut m = HashMap::new();
    m.insert("shell", 256);
    m.insert("grep", 200);
    m.insert("glob", 500);
    m
}

pub fn truncate_output(output: &str, max_chars: usize, mode: TruncationMode) -> String {
    if output.len() <= max_chars {
        return output.to_string();
    }

    match mode {
        TruncationMode::HeadTail => {
            let half = max_chars / 2;
            let head = &output[..half];
            let tail = &output[output.len() - half..];
            format!("{head}{TRUNCATION_WARNING_HEAD_TAIL}\n\n{tail}")
        }
        TruncationMode::Tail => {
            let tail = &output[output.len() - max_chars..];
            format!("{TRUNCATION_WARNING_TAIL}\n\n{tail}")
        }
    }
}

pub fn truncate_lines(output: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= max_lines {
        return output.to_string();
    }

    let half = max_lines / 2;
    let head: Vec<&str> = lines[..half].to_vec();
    let tail: Vec<&str> = lines[lines.len() - half..].to_vec();

    format!(
        "{}{LINE_TRUNCATION_WARNING}\n\n{}",
        head.join("\n"),
        tail.join("\n")
    )
}

pub fn truncate_tool_output(output: &str, tool_name: &str, config: &SessionConfig) -> String {
    let builtin_char_limits = default_char_limits();
    let builtin_line_limits = default_line_limits();

    // Char truncation first
    let char_limit = config
        .tool_output_limits
        .get(tool_name)
        .copied()
        .or_else(|| builtin_char_limits.get(tool_name).copied());

    let after_chars = match char_limit {
        Some(limit) => truncate_output(output, limit, TruncationMode::HeadTail),
        None => output.to_string(),
    };

    // Then line truncation
    let line_limit = config
        .tool_line_limits
        .get(tool_name)
        .copied()
        .or_else(|| builtin_line_limits.get(tool_name).copied());

    match line_limit {
        Some(limit) => truncate_lines(&after_chars, limit),
        None => after_chars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_limit_passthrough_chars() {
        let output = "short output";
        let result = truncate_output(output, 100, TruncationMode::HeadTail);
        assert_eq!(result, output);
    }

    #[test]
    fn under_limit_passthrough_lines() {
        let output = "line1\nline2\nline3";
        let result = truncate_lines(output, 10);
        assert_eq!(result, output);
    }

    #[test]
    fn head_tail_split() {
        let output = "a".repeat(100);
        let result = truncate_output(&output, 40, TruncationMode::HeadTail);
        assert!(result.contains(&"a".repeat(20)));
        assert!(result.contains(TRUNCATION_WARNING_HEAD_TAIL));
    }

    #[test]
    fn tail_mode() {
        let output = format!("{}BBB", "A".repeat(100));
        let result = truncate_output(&output, 10, TruncationMode::Tail);
        assert!(result.contains(TRUNCATION_WARNING_TAIL));
        assert!(result.ends_with("AAAAAAABBB"));
    }

    #[test]
    fn line_truncation_splits_head_tail() {
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        let output = lines.join("\n");
        let result = truncate_lines(&output, 6);
        assert!(result.contains("line 1"));
        assert!(result.contains("line 3"));
        assert!(result.contains("line 18"));
        assert!(result.contains("line 20"));
        assert!(result.contains(LINE_TRUNCATION_WARNING));
    }

    #[test]
    fn char_truncation_before_lines() {
        // Create an output that is large in chars and many lines
        let long_line = "x".repeat(50_000);
        let output = format!("{long_line}\n{long_line}");
        let config = SessionConfig::default();
        let result = truncate_tool_output(&output, "shell", &config);
        // Should have been char-truncated first (30k limit for shell)
        assert!(result.len() < output.len());
    }

    #[test]
    fn config_override_char_limit() {
        let output = "x".repeat(200);
        let mut config = SessionConfig::default();
        config
            .tool_output_limits
            .insert("my_tool".into(), 50);
        let result = truncate_tool_output(&output, "my_tool", &config);
        assert!(result.len() < output.len());
        assert!(result.contains(TRUNCATION_WARNING_HEAD_TAIL));
    }

    #[test]
    fn config_override_line_limit() {
        let lines: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        let output = lines.join("\n");
        let mut config = SessionConfig::default();
        config.tool_line_limits.insert("my_tool".into(), 10);
        let result = truncate_tool_output(&output, "my_tool", &config);
        assert!(result.contains(LINE_TRUNCATION_WARNING));
    }

    #[test]
    fn unknown_tool_no_truncation() {
        let output = "x".repeat(200);
        let config = SessionConfig::default();
        let result = truncate_tool_output(&output, "unknown_tool", &config);
        assert_eq!(result, output);
    }

    #[test]
    fn default_char_limits_match_spec() {
        let limits = default_char_limits();
        assert_eq!(limits.get("read_file"), Some(&50_000));
        assert_eq!(limits.get("shell"), Some(&30_000));
        assert_eq!(limits.get("grep"), Some(&20_000));
        assert_eq!(limits.get("glob"), Some(&20_000));
        assert_eq!(limits.get("edit_file"), Some(&10_000));
        assert_eq!(limits.get("write_file"), Some(&1_000));
    }

    #[test]
    fn default_line_limits_match_spec() {
        let limits = default_line_limits();
        assert_eq!(limits.get("shell"), Some(&256));
        assert_eq!(limits.get("grep"), Some(&200));
        assert_eq!(limits.get("glob"), Some(&500));
    }

    #[test]
    fn exact_limit_not_truncated() {
        let output = "x".repeat(100);
        let result = truncate_output(&output, 100, TruncationMode::HeadTail);
        assert_eq!(result, output);
    }

    #[test]
    fn exact_line_limit_not_truncated() {
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        let output = lines.join("\n");
        let result = truncate_lines(&output, 10);
        assert_eq!(result, output);
    }
}
