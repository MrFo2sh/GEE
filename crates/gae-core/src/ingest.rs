//! Body parsing for `POST /events`: one JSON object, a JSON array, or newline-delimited objects.

use crate::model::Event;

/// The most events one request may carry.
pub const MAX_EVENTS_PER_REQUEST: usize = 10_000;

pub fn parse_events(body: &str) -> Result<Vec<Event>, String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("empty body".into());
    }
    let out = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<Event>>(trimmed).map_err(|e| format!("bad array: {e}"))?
    } else if let Ok(one) = serde_json::from_str::<Event>(trimmed) {
        vec![one]
    } else {
        let mut out = Vec::new();
        for (i, line) in trimmed.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            out.push(serde_json::from_str::<Event>(line).map_err(|e| format!("line {}: {e}", i + 1))?);
        }
        out
    };
    if out.len() > MAX_EVENTS_PER_REQUEST {
        return Err(format!(
            "{} events in one request; the limit is {MAX_EVENTS_PER_REQUEST}",
            out.len()
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_array_and_ndjson() {
        let one = r#"{"type":"unit_down","unit":"a"}"#;
        assert_eq!(parse_events(one).unwrap().len(), 1);
        assert_eq!(parse_events(&format!("[{one},{one}]")).unwrap().len(), 2);
        assert_eq!(parse_events(&format!("{one}\n\n{one}\n{one}\n")).unwrap().len(), 3);
        assert!(parse_events("nope").unwrap_err().starts_with("line 1"));
        assert!(parse_events("  ").is_err());
    }
}
