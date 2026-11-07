//! The small Markdown subset news posts use (port of `src/components/markdown-text.tsx`).
//!
//! Blocks: `#`–`###` headings, `-`/`*` bullets, `>` quotes, paragraphs (lines joined by spaces).
//! Inline: `**bold**`, `*italic*`, `` `code` ``, `[label](https://…|mailto:…)`. Each block is a
//! list of [`Span`]s so a UI can style them; links are only produced for safe URLs, anything else
//! stays literal text.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStyle {
    Plain,
    Bold,
    Italic,
    Code,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub style: SpanStyle,
    pub text: String,
    /// Set for [`SpanStyle::Link`].
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading,
    Bullet,
    Quote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
}

impl Block {
    /// The text with inline markers removed.
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn links(&self) -> impl Iterator<Item = &Span> {
        self.spans.iter().filter(|s| s.style == SpanStyle::Link)
    }

    /// The inline spans as strict CommonMark for a renderer: all text is escaped, so only the
    /// styles and (already vetted) links found by [`parse`] come through. Raw HTML in a post
    /// cannot reach the renderer.
    pub fn to_commonmark(&self) -> String {
        let mut out = String::new();
        for span in &self.spans {
            let text = escape(&span.text);
            match span.style {
                SpanStyle::Plain => out.push_str(&text),
                SpanStyle::Bold => out.push_str(&format!("**{text}**")),
                SpanStyle::Italic => out.push_str(&format!("*{text}*")),
                // Code spans do not process escapes, so emit them as styled plain text instead.
                SpanStyle::Code => out.push_str(&format!("`{}`", span.text.replace('`', "'"))),
                SpanStyle::Link => {
                    let url = span
                        .url
                        .as_deref()
                        .unwrap_or_default()
                        .replace(['(', ')', '<', '>'], "");
                    out.push_str(&format!("[{text}]({url})"));
                }
            }
        }
        out
    }
}

/// Backslash-escapes every ASCII punctuation character (valid for all of them in CommonMark).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn parse(source: &str) -> Vec<Block> {
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();

    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block {
                kind: BlockKind::Paragraph,
                spans: inline(&paragraph.join(" ")),
            });
            paragraph.clear();
        }
    };

    for raw in source.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        if let Some(text) = heading(line) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block {
                kind: BlockKind::Heading,
                spans: inline(text),
            });
        } else if let Some(text) = bullet(line) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block {
                kind: BlockKind::Bullet,
                spans: inline(text),
            });
        } else if let Some(text) = line.strip_prefix('>') {
            flush(&mut paragraph, &mut blocks);
            let text = text.strip_prefix(' ').unwrap_or(text);
            blocks.push(Block {
                kind: BlockKind::Quote,
                spans: inline(text),
            });
        } else {
            paragraph.push(line);
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// `# x`, `## x`, `### x` (a space is required, `####` is a paragraph).
fn heading(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    let text = rest.trim_start();
    (rest.starts_with(char::is_whitespace) && !text.is_empty()).then_some(text)
}

fn bullet(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('-').or_else(|| line.strip_prefix('*'))?;
    let text = rest.trim_start();
    (rest.starts_with(char::is_whitespace) && !text.is_empty()).then_some(text)
}

fn inline(source: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = source;

    while let Some(c) = rest.chars().next() {
        if let Some((span, len)) = token(rest) {
            if !plain.is_empty() {
                spans.push(Span {
                    style: SpanStyle::Plain,
                    text: std::mem::take(&mut plain),
                    url: None,
                });
            }
            spans.push(span);
            rest = &rest[len..];
        } else {
            plain.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    if !plain.is_empty() {
        spans.push(Span {
            style: SpanStyle::Plain,
            text: plain,
            url: None,
        });
    }
    spans
}

/// Recognises one inline token at the start of `s`, in the same priority order as the original
/// regex: bold, code, italic, link. Returns the span and the bytes consumed.
fn token(s: &str) -> Option<(Span, usize)> {
    let delimited = |open: &str, close: char, forbidden: &[char]| -> Option<(String, usize)> {
        let body = s.strip_prefix(open)?;
        let end = body.find(close)?;
        let inner = &body[..end];
        if inner.is_empty() || inner.contains(forbidden) {
            return None;
        }
        Some((inner.to_owned(), open.len() + end + close.len_utf8()))
    };
    let simple = |style, (text, len)| {
        Some((
            Span {
                style,
                text,
                url: None,
            },
            len,
        ))
    };

    if s.starts_with("**")
        && let Some(found) = delimited("**", '*', &['\n'])
    {
        // Must close with "**".
        if s[found.1..].starts_with('*') {
            return simple(SpanStyle::Bold, (found.0, found.1 + 1));
        }
    }
    if let Some(found) = delimited("`", '`', &['\n']) {
        return simple(SpanStyle::Code, found);
    }
    if !s.starts_with("**")
        && let Some(found) = delimited("*", '*', &['\n'])
    {
        return simple(SpanStyle::Italic, found);
    }
    if let Some(body) = s.strip_prefix('[') {
        let close = body.find(']')?;
        let label = &body[..close];
        let after = body[close + 1..].strip_prefix('(')?;
        let end = after.find(')')?;
        let url = &after[..end];
        if label.is_empty() || label.contains('\n') || !is_safe_link(url) {
            return None;
        }
        let len = 1 + close + 2 + end + 1;
        return Some((
            Span {
                style: SpanStyle::Link,
                text: label.to_owned(),
                url: Some(url.to_owned()),
            },
            len,
        ));
    }
    None
}

/// `http(s)://` without whitespace, or a plain `mailto:` with only subject/body/cc/bcc parameters.
pub fn is_safe_link(url: &str) -> bool {
    if url.is_empty() || url.contains(char::is_whitespace) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    {
        return !rest.is_empty();
    }
    let Some(rest) = lower.strip_prefix("mailto:") else {
        return false;
    };
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c));
    let domain_ok = domain.rsplit_once('.').is_some_and(|(host, tld)| {
        !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
            && tld.len() >= 2
            && tld.chars().all(|c| c.is_ascii_alphabetic())
    });
    let query_ok = !rest.contains('?')
        || (!query.is_empty()
            && query.split('&').all(|pair| {
                pair.split_once('=')
                    .is_some_and(|(k, _)| matches!(k, "subject" | "body" | "cc" | "bcc"))
            }));
    local_ok && domain_ok && query_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks() {
        let b = parse(
            "# Patch night\nLine one\nline two\n\n- first\n* second\n> quoted\n#### not a heading",
        );
        let kinds: Vec<_> = b.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [
                BlockKind::Heading,
                BlockKind::Paragraph,
                BlockKind::Bullet,
                BlockKind::Bullet,
                BlockKind::Quote,
                BlockKind::Paragraph
            ]
        );
        assert_eq!(b[1].plain_text(), "Line one line two");
        assert_eq!(b[4].plain_text(), "quoted");
    }

    #[test]
    fn inline_styles() {
        let spans =
            inline("A **big** and *small* `cmd` [site](https://x.test) [bad](javascript:alert(1))");
        let styles: Vec<_> = spans.iter().map(|s| (s.style, s.text.as_str())).collect();
        assert_eq!(
            styles,
            [
                (SpanStyle::Plain, "A "),
                (SpanStyle::Bold, "big"),
                (SpanStyle::Plain, " and "),
                (SpanStyle::Italic, "small"),
                (SpanStyle::Plain, " "),
                (SpanStyle::Code, "cmd"),
                (SpanStyle::Plain, " "),
                (SpanStyle::Link, "site"),
                (SpanStyle::Plain, " [bad](javascript:alert(1))"),
            ]
        );
        assert_eq!(spans[7].url.as_deref(), Some("https://x.test"));
    }

    #[test]
    fn unclosed_markers_stay_literal() {
        assert_eq!(inline("2 * 3 = 6")[0].text, "2 * 3 = 6");
        assert_eq!(inline("**open")[0].text, "**open");
    }

    #[test]
    fn commonmark_escapes_everything_but_known_styles() {
        let b = &parse("Hi <b>x</b> **bold** 1. [site](https://x.test/a_(b)) *_*")[0];
        assert_eq!(
            b.to_commonmark(),
            r"Hi \<b\>x\<\/b\> **bold** 1\. [site](https://x.test/a_b)\) *\_*"
        );
    }

    #[test]
    fn safe_links() {
        assert!(is_safe_link(
            "https://www.frontierwinggaming.network/servers"
        ));
        assert!(is_safe_link(
            "mailto:staff@frontierwinggaming.network?subject=Hi"
        ));
        assert!(!is_safe_link("mailto:staff@x.network?evil=1"));
        assert!(!is_safe_link("steam://connect/1.2.3.4"));
        assert!(!is_safe_link("https://x.test/a b"));
    }
}
