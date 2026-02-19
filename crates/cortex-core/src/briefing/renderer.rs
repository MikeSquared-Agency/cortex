use super::{Briefing, BriefingSection};

pub trait BriefingRenderer {
    fn render(&self, briefing: &Briefing) -> String;
}

pub struct MarkdownRenderer {
    pub max_chars: usize,
}

pub struct CompactRenderer {
    pub max_chars: usize,
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self { max_chars: 8000 }
    }
}

impl Default for CompactRenderer {
    fn default() -> Self {
        Self { max_chars: 8000 }
    }
}

/// Truncate `s` to at most `max_chars` Unicode scalar values.
/// Appends " [truncated]" when there is room; otherwise hard-truncates.
fn truncate(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }
    const SUFFIX: &str = " [truncated]";
    const SUFFIX_LEN: usize = 14; // " [truncated]".chars().count()
    let keep = if max_chars > SUFFIX_LEN {
        max_chars - SUFFIX_LEN
    } else {
        // Not enough room for the annotation — hard-truncate to max_chars.
        let byte_end = s
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        return s[..byte_end].to_string();
    };
    let byte_end = s
        .char_indices()
        .nth(keep)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}{}", &s[..byte_end], SUFFIX)
}

/// Truncate a body preview to `max_chars` characters, appending "..." when cut.
fn body_preview(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let byte_end = s
        .char_indices()
        .nth(keep)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}...", &s[..byte_end])
}

fn render_section_markdown(section: &BriefingSection) -> String {
    let mut out = format!("## {}\n\n", section.title);
    for node in &section.nodes {
        let preview = body_preview(&node.data.body, 200);
        out.push_str(&format!("- **{}**: {}\n", node.data.title, preview));
    }
    out
}

fn render_section_compact(section: &BriefingSection) -> String {
    let mut out = format!("## {}\n", section.title);
    for node in &section.nodes {
        out.push_str(&format!("- {}\n", node.data.title));
    }
    out
}

impl BriefingRenderer for MarkdownRenderer {
    fn render(&self, briefing: &Briefing) -> String {
        let mut out = format!(
            "# Briefing: {}\n_Generated: {}_\n\n",
            briefing.agent_id,
            briefing.generated_at.format("%Y-%m-%d %H:%M UTC")
        );
        for section in &briefing.sections {
            out.push_str(&render_section_markdown(section));
            out.push('\n');
        }
        truncate(&out, self.max_chars)
    }
}

impl BriefingRenderer for CompactRenderer {
    fn render(&self, briefing: &Briefing) -> String {
        let mut out = format!("# {}\n", briefing.agent_id);
        for section in &briefing.sections {
            out.push_str(&render_section_compact(section));
        }
        truncate(&out, self.max_chars)
    }
}
