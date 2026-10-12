use super::error::{Error, Result};

const OPEN: &str = "{{";
const CLOSE: &str = "}}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Plain(String),
    Copy(String),
}

/// The sign's text: a sentence in which `{{value}}` marks a value the human can copy with one
/// click, e.g. "Franz, copy {{sudo apt upgrade}} and paste it here".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignText {
    pub segments: Vec<Segment>,
}

impl SignText {
    /// Splits at `{{` and `}}`. Unbalanced or empty markers are bad input, never guessed.
    pub fn parse(raw: &str) -> Result<Self> {
        let mut segments = Vec::new();
        let mut rest = raw;
        while let Some(start) = rest.find(OPEN) {
            append_plain(&rest[..start], &mut segments)?;
            let after_open = &rest[start + OPEN.len()..];
            let end = after_open.find(CLOSE).ok_or_else(|| {
                Error::bad_input("--text has '{{' without '}}': write {{value}} for a copy button")
            })?;
            let value = after_open[..end].trim();
            if value.is_empty() || value.contains(OPEN) {
                return Err(Error::bad_input(
                    "--text has an empty or nested {{ }}: write {{value}} for a copy button",
                ));
            }
            segments.push(Segment::Copy(value.to_owned()));
            rest = &after_open[end + CLOSE.len()..];
        }
        append_plain(rest, &mut segments)?;
        Ok(Self { segments })
    }

    /// The sentence without braces, values included: what `--say` speaks and the registry records.
    pub fn plain(&self) -> String {
        self.segments
            .iter()
            .map(|segment| match segment {
                Segment::Plain(text) | Segment::Copy(text) => text.as_str(),
            })
            .collect()
    }
}

fn append_plain(text: &str, segments: &mut Vec<Segment>) -> Result<()> {
    if text.contains(CLOSE) {
        return Err(Error::bad_input(
            "--text has '}}' without '{{': write {{value}} for a copy button",
        ));
    }
    if !text.is_empty() {
        segments.push(Segment::Plain(text.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_become_copy_segments() {
        let text = SignText::parse("Franz, copy {{ 482913 }} here").unwrap();
        assert_eq!(
            text.segments,
            vec![
                Segment::Plain("Franz, copy ".into()),
                Segment::Copy("482913".into()),
                Segment::Plain(" here".into())
            ]
        );
        assert_eq!(text.plain(), "Franz, copy 482913 here");
        assert_eq!(
            SignText::parse("{{a}}{{b}}").unwrap().segments,
            vec![Segment::Copy("a".into()), Segment::Copy("b".into())]
        );
    }

    #[test]
    fn unbalanced_or_empty_markers_are_bad_input() {
        for raw in [
            "Copy {{this",
            "Copy this}}",
            "Copy {{}}",
            "{{ }}",
            "{{a {{b}} c}}",
        ] {
            let error = SignText::parse(raw).unwrap_err();
            assert_eq!(error.code, 2, "{raw}");
            assert!(error.message.contains("{{value}}"), "{raw}");
        }
    }
}
