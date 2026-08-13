use serde_json::Value;

/// Required version of persisted profile projections.
pub const PROJECTION_VERSION: u32 = 1;

/// Title-specific alias for consumers that expose per-projection diagnostics.
pub const TITLE_PROJECTION_VERSION: u32 = PROJECTION_VERSION;

/// Extract a safe, human-readable title candidate from one user-message payload.
///
/// Structured content is traversed for textual leaves. It is never serialized.
pub fn project_user_message(value: &Value) -> Option<String> {
    let mut candidates = Vec::new();
    collect_text(value, &mut candidates);
    candidates.into_iter().filter_map(sanitize).next()
}

fn collect_text<'a>(value: &'a Value, output: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => output.push(text),
        Value::Array(items) => {
            for item in items {
                collect_text(item, output);
            }
        }
        Value::Object(object) => {
            for key in ["text", "content", "message", "input_text"] {
                if let Some(value) = object.get(key) {
                    collect_text(value, output);
                }
            }
        }
        _ => {}
    }
}

fn sanitize(input: &str) -> Option<String> {
    let without_blocks = strip_xml_blocks(
        input,
        &[
            "recommended_plugins",
            "skill",
            "app-context",
            "environment_context",
        ],
    );
    let lines = without_blocks.lines().map(str::trim).collect::<Vec<_>>();
    let request_start = lines.iter().position(|line| is_request_heading(line));
    let meaningful = if let Some(position) = request_start {
        lines[position + 1..].join(" ")
    } else if lines.iter().any(|line| is_injected_heading(line)) {
        String::new()
    } else {
        lines.join(" ")
    };
    let collapsed = meaningful.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then(|| truncate_display_width(&collapsed, 80))
}

fn is_request_heading(line: &str) -> bool {
    let normalized = line.trim_start_matches('#').trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "my request:" | "my request")
}

fn strip_xml_blocks(input: &str, tags: &[&str]) -> String {
    let mut result = input.to_owned();
    for tag in tags {
        loop {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            let Some(start) = result.find(&open) else {
                break;
            };
            let end = result[start + open.len()..]
                .find(&close)
                .map(|offset| start + open.len() + offset + close.len())
                .unwrap_or(result.len());
            result.replace_range(start..end, "\n");
        }
    }
    result
}

fn is_injected_heading(line: &str) -> bool {
    let normalized = line.trim_start_matches('#').trim().to_ascii_lowercase();
    normalized.starts_with("agents.md instructions")
        || normalized.starts_with("environment context")
        || normalized.starts_with("app context")
        || normalized.starts_with("files mentioned by the user")
        || normalized.starts_with("response annotations")
}

fn truncate_display_width(input: &str, max_width: usize) -> String {
    let width = input.chars().map(char_width).sum::<usize>();
    if width <= max_width {
        return input.to_owned();
    }
    let mut output = String::new();
    let mut used = 0;
    for character in input.chars() {
        let next = char_width(character);
        if used + next + 1 > max_width {
            break;
        }
        output.push(character);
        used += next;
    }
    output.push('…');
    output
}

fn char_width(character: char) -> usize {
    match character as u32 {
        0x1100..=0x115f
        | 0x2329..=0x232a
        | 0x2e80..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe10..=0xfe19
        | 0xfe30..=0xfe6f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6
        | 0x1f300..=0x1faff
        | 0x20000..=0x3fffd => 2,
        _ => 1,
    }
}
