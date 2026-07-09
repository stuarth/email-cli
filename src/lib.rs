use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
};

use anyhow::{Context, Result, anyhow};
use mail_parser::{
    Address, ContentType, DateTime, HeaderName, HeaderValue, Message, MessageParser, MessagePart,
    MimeHeaders,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: &str = "1.1";
pub const DEFAULT_MAX_BODY_BYTES: usize = 65_536;

/// Headers included in JSON output by default. Everything else (transport,
/// authentication, and vendor headers) is metadata agents rarely read and is
/// available with `--headers all`.
const STANDARD_HEADERS: &[&str] = &[
    "from",
    "sender",
    "reply-to",
    "to",
    "cc",
    "bcc",
    "subject",
    "date",
    "message-id",
    "in-reply-to",
    "references",
    "mime-version",
    "content-type",
    "list-id",
    "list-unsubscribe",
    "auto-submitted",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuoteMode {
    Keep,
    Collapse,
    Drop,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeaderScope {
    #[default]
    Standard,
    All,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub include_html: bool,
    pub max_body_bytes: usize,
    pub quotes: QuoteMode,
    pub headers: HeaderScope,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            include_html: false,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            quotes: QuoteMode::Keep,
            headers: HeaderScope::Standard,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceDto {
    pub path: String,
    pub size_bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddressDto {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageFieldsDto {
    pub node_id: String,
    pub message_id: Option<String>,
    pub date: Option<String>,
    #[serde(skip_serializing)]
    pub date_timestamp: Option<i64>,
    pub date_original: Option<String>,
    pub subject: Option<String>,
    pub from: Vec<AddressDto>,
    pub to: Vec<AddressDto>,
    pub cc: Vec<AddressDto>,
    pub bcc: Vec<AddressDto>,
    pub reply_to: Vec<AddressDto>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadHintDto {
    pub parent_message_id: Option<String>,
    pub root_message_id: Option<String>,
    pub references: Vec<String>,
    pub base_subject: Option<String>,
    pub is_reply: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BodyDto {
    pub text: String,
    pub html: Option<String>,
    pub html_included: bool,
    pub html_available: bool,
    pub text_source: String,
    pub text_part_id: Option<String>,
    pub alternatives: Vec<BodyAlternativeDto>,
    pub fragments: Vec<BodyFragmentDto>,
    pub truncation: TruncationDto,
}

#[derive(Clone, Debug, Serialize)]
pub struct BodyAlternativeDto {
    pub part_id: String,
    pub kind: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub same_as: Option<String>,
    pub decoded_size_bytes: usize,
    pub decoded_sha256: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BodyFragmentDto {
    pub id: String,
    pub kind: String,
    pub quote_depth: usize,
    pub part_id: String,
    pub byte_range: [usize; 2],
    pub sha256: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TruncationDto {
    pub max_body_bytes: usize,
    pub truncated: bool,
    pub omitted_fragment_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HeaderDto {
    pub name: String,
    pub value: String,
    pub raw: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartDto {
    pub part_id: String,
    pub kind: String,
    pub content_type: String,
    pub filename: Option<String>,
    pub safe_filename: Option<String>,
    pub disposition: Option<String>,
    pub content_id: Option<String>,
    pub decoded_size_bytes: usize,
    pub decoded_sha256: String,
    pub extractable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct NestedMessageDto {
    pub part_id: String,
    pub message_id: Option<String>,
    pub subject: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticDto {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub json_path: Option<String>,
    pub location: Option<String>,
}

impl DiagnosticDto {
    pub fn info(code: &str, message: impl Into<String>, json_path: Option<&str>) -> Self {
        Self {
            code: code.to_string(),
            severity: "info".to_string(),
            message: message.into(),
            json_path: json_path.map(str::to_string),
            location: None,
        }
    }

    pub fn warning(
        code: &str,
        message: impl Into<String>,
        json_path: Option<&str>,
        location: Option<&str>,
    ) -> Self {
        Self {
            code: code.to_string(),
            severity: "warning".to_string(),
            message: message.into(),
            json_path: json_path.map(str::to_string),
            location: location.map(str::to_string),
        }
    }

    pub fn error(
        code: &str,
        message: impl Into<String>,
        json_path: Option<&str>,
        location: Option<&str>,
    ) -> Self {
        Self {
            code: code.to_string(),
            severity: "error".to_string(),
            message: message.into(),
            json_path: json_path.map(str::to_string),
            location: location.map(str::to_string),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageDto {
    pub schema_version: String,
    pub source: SourceDto,
    pub message: MessageFieldsDto,
    pub thread: ThreadHintDto,
    pub body: BodyDto,
    pub headers: Vec<HeaderDto>,
    pub headers_omitted: usize,
    pub parts: Vec<PartDto>,
    pub nested_messages: Vec<NestedMessageDto>,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessagesEnvelopeDto {
    pub schema_version: String,
    pub messages: Vec<MessageDto>,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadEnvelopeDto {
    pub schema_version: String,
    pub sources: Vec<SourceDto>,
    pub threads: Vec<ThreadDto>,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadDto {
    pub thread_id: String,
    pub root_message_id: Option<String>,
    pub base_subject: Option<String>,
    pub participants: Vec<AddressDto>,
    pub time_range: TimeRangeDto,
    pub nodes: Vec<ThreadNodeDto>,
    pub timeline: Vec<String>,
    pub missing_parent_message_ids: Vec<String>,
    pub duplicate_message_ids: Vec<String>,
    pub diagnostics: Vec<DiagnosticDto>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TimeRangeDto {
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadNodeDto {
    pub node_id: String,
    pub message_id: Option<String>,
    pub parent_node_id: Option<String>,
    pub child_node_ids: Vec<String>,
    pub depth: usize,
    pub link_confidence: String,
    pub message: MessageDto,
}

pub fn parse_message_bytes(
    path: impl Into<String>,
    raw: &[u8],
    options: RenderOptions,
) -> Result<MessageDto> {
    let path = path.into();
    let source = SourceDto {
        path,
        size_bytes: raw.len(),
        sha256: sha256_hex(raw),
    };

    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow!("input does not contain a parseable RFC 822/RFC 5322 message"))?;

    Ok(build_message_dto(&message, source, options))
}

pub fn extract_part_bytes(raw: &[u8], part_id: &str) -> Result<Vec<u8>> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow!("input does not contain a parseable RFC 822/RFC 5322 message"))?;
    let paths = part_paths(&message);
    let part = paths
        .iter()
        .position(|path| path == part_id)
        .and_then(|idx| message.parts.get(idx))
        .ok_or_else(|| anyhow!("part id {part_id:?} was not found"))?;

    if part.is_multipart() {
        return Err(anyhow!(
            "part id {part_id:?} is multipart and has no decoded bytes"
        ));
    }

    Ok(part.contents().to_vec())
}

pub fn build_thread_envelope(
    mut messages: Vec<MessageDto>,
    subject_fallback: bool,
) -> ThreadEnvelopeDto {
    messages.sort_by_key(message_sort_key);
    ensure_unique_node_ids(&mut messages);

    let sources = messages
        .iter()
        .map(|m| m.source.clone())
        .collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    let mut duplicate_ids = BTreeSet::new();
    let mut id_to_indexes = HashMap::<String, Vec<usize>>::new();

    for (idx, message) in messages.iter().enumerate() {
        if let Some(message_id) = &message.message.message_id {
            let entries = id_to_indexes.entry(message_id.clone()).or_default();
            if !entries.is_empty() {
                duplicate_ids.insert(message_id.clone());
            }
            entries.push(idx);
        }
    }

    if !duplicate_ids.is_empty() {
        diagnostics.push(DiagnosticDto::warning(
            "DUPLICATE_MESSAGE_ID",
            "One or more Message-ID values occur more than once.",
            Some("$.threads"),
            None,
        ));
    }

    let components = thread_components(&messages, &id_to_indexes, subject_fallback);

    let threads = components
        .into_iter()
        .map(|component| build_thread(component, &messages, &id_to_indexes, &duplicate_ids))
        .collect();

    ThreadEnvelopeDto {
        schema_version: SCHEMA_VERSION.to_string(),
        sources,
        threads,
        diagnostics,
    }
}

pub fn build_messages_envelope(
    mut messages: Vec<MessageDto>,
    diagnostics: Vec<DiagnosticDto>,
) -> MessagesEnvelopeDto {
    ensure_unique_node_ids(&mut messages);

    MessagesEnvelopeDto {
        schema_version: SCHEMA_VERSION.to_string(),
        messages,
        diagnostics,
    }
}

pub fn render_message_text(message: &MessageDto, quote_mode: QuoteMode) -> String {
    let mut out = String::new();
    let subject = message.message.subject.as_deref().unwrap_or("(no subject)");
    out.push_str(&format!("Subject: {subject}\n"));
    if let Some(date) = &message.message.date {
        out.push_str(&format!("Date: {date}\n"));
    }
    if !message.message.from.is_empty() {
        out.push_str(&format!(
            "From: {}\n",
            render_addresses(&message.message.from)
        ));
    }
    if !message.message.to.is_empty() {
        out.push_str(&format!("To: {}\n", render_addresses(&message.message.to)));
    }
    out.push('\n');
    out.push_str(&render_body_text(&message.body, quote_mode));

    let visible_parts = visible_text_parts(&message.parts);
    if !visible_parts.is_empty() {
        out.push_str("\n\nAttachments and parts:\n");
        for part in visible_parts {
            out.push_str(&format!(
                "- {} {} {} {} {} bytes sha256:{}\n",
                part.part_id,
                part.filename.as_deref().unwrap_or("(unnamed)"),
                part.content_type,
                part.kind,
                part.decoded_size_bytes,
                part.decoded_sha256
            ));
        }
    }

    out
}

pub fn render_thread_text(envelope: &ThreadEnvelopeDto, quote_mode: QuoteMode) -> String {
    let mut out = String::new();
    for thread in &envelope.threads {
        out.push_str(&format!(
            "Thread: {}\n\n",
            thread.base_subject.as_deref().unwrap_or("(no subject)")
        ));

        for node_id in &thread.timeline {
            let Some(node) = thread.nodes.iter().find(|node| &node.node_id == node_id) else {
                continue;
            };
            let message = &node.message;
            let date = message.message.date.as_deref().unwrap_or("(no date)");
            let from = if message.message.from.is_empty() {
                "(unknown sender)".to_string()
            } else {
                render_addresses(&message.message.from)
            };
            out.push_str(&format!("[{}] {} {}\n", node.node_id, date, from));
            out.push_str(&render_body_text(&message.body, quote_mode));

            let visible_parts = visible_text_parts(&message.parts);
            if !visible_parts.is_empty() {
                out.push_str("\n\nAttachments and parts:\n");
                for part in visible_parts {
                    out.push_str(&format!(
                        "- {} {} {} {} {} bytes sha256:{}\n",
                        part.part_id,
                        part.filename.as_deref().unwrap_or("(unnamed)"),
                        part.content_type,
                        part.kind,
                        part.decoded_size_bytes,
                        part.decoded_sha256
                    ));
                }
            }

            out.push_str("\n\n");
        }
    }

    out.trim_end().to_string()
}

fn visible_text_parts(parts: &[PartDto]) -> Vec<&PartDto> {
    parts.iter().filter(|part| part.kind != "body").collect()
}

pub fn read_file_or_stdin(path: Option<&str>) -> Result<(String, Vec<u8>)> {
    match path {
        None | Some("-") => {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .context("failed to read message from stdin")?;
            Ok(("-".to_string(), buf))
        }
        Some(path) => {
            let raw = std::fs::read(path).with_context(|| format!("failed to read {path}"))?;
            Ok((path.to_string(), raw))
        }
    }
}

pub fn read_required_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))
}

fn build_message_dto(
    message: &Message<'_>,
    source: SourceDto,
    options: RenderOptions,
) -> MessageDto {
    let mut diagnostics = Vec::new();
    let paths = part_paths(message);
    let source_hash = source.sha256.clone();

    let message_id = message.message_id().and_then(normalize_message_id);
    let references = ids_from_header(message.references());
    let in_reply_to = ids_from_header(message.in_reply_to()).into_iter().next();
    let parent_message_id = in_reply_to.clone().or_else(|| references.last().cloned());
    let root_message_id = references.first().cloned();
    let subject = message.subject().map(str::to_string);
    let base_subject = message
        .thread_name()
        .map(str::to_string)
        .or_else(|| subject.clone());
    let node_seed = message_id
        .as_ref()
        .map(|message_id| format!("{message_id}:{}", source.sha256))
        .unwrap_or_else(|| source_hash.clone());
    let node_id = format!("msg_{}", short_sha256(node_seed.as_bytes()));
    let is_reply = parent_message_id.is_some()
        || subject
            .as_deref()
            .is_some_and(|subject| subject.trim_start().to_ascii_lowercase().starts_with("re:"));

    let parsed_date = message.date();
    let date = parsed_date.map(|date| DateTime::from_timestamp(date.to_timestamp()).to_rfc3339());
    let date_timestamp = parsed_date.map(|date| date.to_timestamp());
    let date_original = message
        .header_raw(HeaderName::Date)
        .map(|raw| raw_header_value(raw, "Date"));

    let text_part_idx = message.text_body.first().copied().map(|idx| idx as usize);
    let text_part_id = text_part_idx
        .and_then(|idx| paths.get(idx).cloned())
        .unwrap_or_else(|| "1".to_string());
    let full_text = message
        .body_text(0)
        .map(|text| text.into_owned())
        .unwrap_or_default();
    let text_source = text_part_idx
        .and_then(|idx| message.parts.get(idx))
        .map(|part| {
            if part.is_text_html() {
                "html"
            } else if part.is_text() {
                "text"
            } else {
                "none"
            }
        })
        .unwrap_or("none")
        .to_string();
    let html_available =
        message.html_body_count() > 0 || message.parts.iter().any(|part| part.is_text_html());
    if text_source == "html" && !options.include_html {
        diagnostics.push(DiagnosticDto::info(
            "HTML_CONVERTED_TO_TEXT",
            "HTML body content was converted to text; pass --html to include raw HTML.",
            Some("$.body"),
        ));
    }
    let (text, fragments, truncation) = build_body_text(
        &full_text,
        &text_part_id,
        options.max_body_bytes,
        &mut diagnostics,
    );
    let body_text_part_id = text_part_idx.and_then(|idx| paths.get(idx).cloned());
    if text_source == "text" && text_looks_like_html(&text) {
        diagnostics.push(DiagnosticDto::info(
            "TEXT_BODY_CONTAINS_HTML",
            "Plain-text body appears to contain raw HTML markup; consider the HTML alternative when html_available is true.",
            Some("$.body"),
        ));
    }
    let mut alternatives = build_body_alternatives(message, &paths, options);
    let html = if options.include_html {
        message.body_html(0).map(|html| html.into_owned())
    } else {
        None
    };

    // The chosen body content is already in body.text / body.html; repeating it
    // in alternatives doubles the payload for consumers that pay per token.
    // Both branches match by source part rather than content equality, so the
    // dedup still holds when truncation makes the serialized copies differ.
    let body_html_part_id = message
        .html_body
        .first()
        .and_then(|idx| paths.get(*idx as usize).cloned());
    for alternative in &mut alternatives {
        if alternative.text.is_some()
            && Some(alternative.part_id.as_str()) == body_text_part_id.as_deref()
        {
            alternative.text = None;
            alternative.same_as = Some("body.text".to_string());
        } else if alternative.html.is_some()
            && html.is_some()
            && Some(alternative.part_id.as_str()) == body_html_part_id.as_deref()
        {
            alternative.html = None;
            alternative.same_as = Some("body.html".to_string());
            // body.html is emitted untruncated, so the pointed-to content is
            // complete even when this entry's copy would have been cut.
            alternative.truncated = false;
        }
    }

    let all_headers = message
        .headers()
        .iter()
        .map(|header| {
            let raw = message
                .raw_message
                .get(header.offset_start as usize..header.offset_end as usize)
                .and_then(|raw| std::str::from_utf8(raw).ok())
                .unwrap_or_default()
                .to_string();
            HeaderDto {
                name: header.name.as_str().to_string(),
                value: header_value_to_string(&header.value),
                raw,
            }
        })
        .collect::<Vec<_>>();
    let (headers, headers_omitted) = match options.headers {
        HeaderScope::All => (all_headers, 0),
        HeaderScope::Standard => {
            let total = all_headers.len();
            let kept = all_headers
                .into_iter()
                .filter(|header| {
                    STANDARD_HEADERS.contains(&header.name.to_ascii_lowercase().as_str())
                })
                .collect::<Vec<_>>();
            let omitted = total - kept.len();
            (kept, omitted)
        }
    };

    let attachment_indexes = message
        .attachments
        .iter()
        .map(|idx| *idx as usize)
        .collect::<HashSet<_>>();
    let mut parts = Vec::new();
    let mut nested_messages = Vec::new();

    for (idx, part) in message.parts.iter().enumerate() {
        if idx == 0 && !root_part_should_be_listed(part) {
            continue;
        }
        if part.is_multipart() {
            continue;
        }

        let part_id = paths.get(idx).cloned().unwrap_or_else(|| idx.to_string());
        if part.is_encoding_problem {
            diagnostics.push(DiagnosticDto::warning(
                "PART_ENCODING_PROBLEM",
                "mail-parser reported an encoding problem for this MIME part.",
                Some("$.parts"),
                Some(&part_id),
            ));
        }

        if let Some(nested) = part.message() {
            nested_messages.push(NestedMessageDto {
                part_id: part_id.clone(),
                message_id: nested.message_id().and_then(normalize_message_id),
                subject: nested.subject().map(str::to_string),
            });
        }

        parts.push(part_dto(part, &part_id, attachment_indexes.contains(&idx)));
    }

    MessageDto {
        schema_version: SCHEMA_VERSION.to_string(),
        source,
        message: MessageFieldsDto {
            node_id,
            message_id,
            date,
            date_timestamp,
            date_original,
            subject,
            from: address_list(message.from()),
            to: address_list(message.to()),
            cc: address_list(message.cc()),
            bcc: address_list(message.bcc()),
            reply_to: address_list(message.reply_to()),
            in_reply_to,
            references: references.clone(),
        },
        thread: ThreadHintDto {
            parent_message_id,
            root_message_id,
            references,
            base_subject,
            is_reply,
        },
        body: BodyDto {
            text,
            html,
            html_included: options.include_html,
            html_available,
            text_source,
            text_part_id: body_text_part_id,
            alternatives,
            fragments,
            truncation,
        },
        headers,
        headers_omitted,
        parts,
        nested_messages,
        diagnostics,
    }
}

fn part_dto(part: &MessagePart<'_>, part_id: &str, is_attachment: bool) -> PartDto {
    let filename = part.attachment_name().map(str::to_string);
    let disposition = part.content_disposition().map(content_type_name);
    let is_attachment = is_attachment
        || disposition
            .as_deref()
            .is_some_and(|d| d.eq_ignore_ascii_case("attachment"));
    let kind = if is_attachment {
        "attachment"
    } else if part.is_message() {
        "message"
    } else if disposition
        .as_deref()
        .is_some_and(|d| d.eq_ignore_ascii_case("inline"))
    {
        "inline"
    } else if part.is_text() {
        "body"
    } else {
        "part"
    };

    PartDto {
        part_id: part_id.to_string(),
        kind: kind.to_string(),
        content_type: part
            .content_type()
            .map(content_type_name)
            .unwrap_or_else(|| inferred_content_type(part).to_string()),
        safe_filename: filename.as_deref().map(safe_filename),
        filename,
        disposition,
        content_id: part.content_id().map(str::to_string),
        decoded_size_bytes: part.contents().len(),
        decoded_sha256: sha256_hex(part.contents()),
        extractable: !part.is_multipart(),
    }
}

fn root_part_should_be_listed(part: &MessagePart<'_>) -> bool {
    if part.is_multipart() {
        return false;
    }

    part.is_message()
        || part.attachment_name().is_some()
        || part
            .content_disposition()
            .is_some_and(|disposition| disposition.c_type.eq_ignore_ascii_case("attachment"))
}

fn inferred_content_type(part: &MessagePart<'_>) -> &'static str {
    if part.is_text_html() {
        "text/html"
    } else if part.is_text() {
        "text/plain"
    } else if part.is_message() {
        "message/rfc822"
    } else {
        "application/octet-stream"
    }
}

fn content_type_name(content_type: &ContentType<'_>) -> String {
    match &content_type.c_subtype {
        Some(subtype) => format!("{}/{}", content_type.c_type, subtype),
        None => content_type.c_type.to_string(),
    }
}

fn build_body_alternatives(
    message: &Message<'_>,
    paths: &[String],
    options: RenderOptions,
) -> Vec<BodyAlternativeDto> {
    let mut alternatives = Vec::new();

    for (pos, part_idx) in message.text_body.iter().copied().enumerate() {
        let Some(part) = message.parts.get(part_idx as usize) else {
            continue;
        };
        let Some(text) = message.body_text(pos).map(|text| text.into_owned()) else {
            continue;
        };
        let part_id = paths
            .get(part_idx as usize)
            .cloned()
            .unwrap_or_else(|| (part_idx + 1).to_string());
        let kind = if part.is_text_html() {
            "html_text"
        } else {
            "text"
        };
        alternatives.push(body_alternative(
            part_id,
            kind,
            Some(text),
            None,
            options.max_body_bytes,
            options.include_html,
        ));
    }

    for (pos, part_idx) in message.html_body.iter().copied().enumerate() {
        let Some(html) = message.body_html(pos).map(|html| html.into_owned()) else {
            continue;
        };
        let part_id = paths
            .get(part_idx as usize)
            .cloned()
            .unwrap_or_else(|| (part_idx + 1).to_string());
        alternatives.push(body_alternative(
            part_id,
            "html",
            None,
            Some(html),
            options.max_body_bytes,
            options.include_html,
        ));
    }

    alternatives
}

fn body_alternative(
    part_id: String,
    kind: &str,
    text: Option<String>,
    html: Option<String>,
    max_body_bytes: usize,
    include_html: bool,
) -> BodyAlternativeDto {
    let content = text.as_deref().or(html.as_deref()).unwrap_or_default();
    let decoded_size_bytes = content.len();
    let decoded_sha256 = sha256_hex(content.as_bytes());
    let (text, text_truncated) = text
        .map(|text| truncate_to_max_body_bytes(&text, max_body_bytes))
        .map(|(text, truncated)| (Some(text), truncated))
        .unwrap_or((None, false));
    let (html, html_truncated) = html
        .filter(|_| include_html)
        .map(|html| truncate_to_max_body_bytes(&html, max_body_bytes))
        .map(|(html, truncated)| (Some(html), truncated))
        .unwrap_or((None, false));

    BodyAlternativeDto {
        part_id,
        kind: kind.to_string(),
        text,
        html,
        same_as: None,
        decoded_size_bytes,
        decoded_sha256,
        truncated: text_truncated || html_truncated,
    }
}

fn truncate_to_max_body_bytes(value: &str, max_body_bytes: usize) -> (String, bool) {
    if value.len() <= max_body_bytes {
        return (value.to_string(), false);
    }

    let end = utf8_boundary_at_or_before(value, max_body_bytes);
    (value[..end].to_string(), true)
}

fn build_body_text(
    full_text: &str,
    part_id: &str,
    max_body_bytes: usize,
    diagnostics: &mut Vec<DiagnosticDto>,
) -> (String, Vec<BodyFragmentDto>, TruncationDto) {
    let full_fragments = quote_fragments(full_text, part_id);
    let truncated = full_text.len() > max_body_bytes;
    let text_end = if truncated {
        diagnostics.push(DiagnosticDto::info(
            "BODY_TRUNCATED",
            "Body text was truncated by --max-body-bytes.",
            Some("$.body"),
        ));
        utf8_boundary_at_or_before(full_text, max_body_bytes)
    } else {
        full_text.len()
    };
    let text = full_text[..text_end].to_string();

    let mut fragments = Vec::new();
    let mut omitted_fragment_ids = Vec::new();

    for fragment in full_fragments {
        if fragment.byte_range[0] >= text_end {
            omitted_fragment_ids.push(fragment.id);
            continue;
        }

        let mut clipped = fragment;
        if clipped.byte_range[1] > text_end {
            clipped.byte_range[1] = text_end;
            clipped.truncated = true;
            let bytes = &full_text.as_bytes()[clipped.byte_range[0]..clipped.byte_range[1]];
            clipped.sha256 = sha256_hex(bytes);
            clipped.id = format!(
                "frag_{}",
                short_sha256(
                    format!(
                        "{part_id}:{}:{}:{}",
                        clipped.byte_range[0], clipped.byte_range[1], clipped.sha256
                    )
                    .as_bytes()
                )
            );
        }
        fragments.push(clipped);
    }

    (
        text,
        fragments,
        TruncationDto {
            max_body_bytes,
            truncated,
            omitted_fragment_ids,
        },
    )
}

fn quote_fragments(text: &str, part_id: &str) -> Vec<BodyFragmentDto> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut runs = Vec::<(String, usize, usize, usize)>::new();
    let mut run_kind = String::new();
    let mut run_depth = 0usize;
    let mut run_start = 0usize;
    let mut run_end = 0usize;
    let mut initialized = false;

    let lines = lines_with_offsets(text);
    let tail_start = tail_quote_start(&lines);
    for (idx, (start, line)) in lines.iter().enumerate() {
        let (start, line) = (*start, *line);
        let end = start + line.len();
        let in_tail = tail_start.is_some_and(|tail| idx >= tail);
        let depth = quote_depth(line);
        let quoted = in_tail || depth > 0 || is_reply_attribution(line);
        let kind = if quoted { "quoted" } else { "fresh" };
        let depth = if quoted { depth.max(1) } else { 0 };

        if !initialized {
            run_kind = kind.to_string();
            run_depth = depth;
            run_start = start;
            run_end = end;
            initialized = true;
            continue;
        }

        if run_kind == kind && run_depth == depth {
            run_end = end;
        } else {
            runs.push((run_kind, run_depth, run_start, run_end));
            run_kind = kind.to_string();
            run_depth = depth;
            run_start = start;
            run_end = end;
        }
    }

    if initialized {
        runs.push((run_kind, run_depth, run_start, run_end));
    }

    runs.into_iter()
        .map(|(kind, quote_depth, start, end)| {
            let bytes = &text.as_bytes()[start..end];
            BodyFragmentDto {
                id: format!(
                    "frag_{}",
                    short_sha256(
                        format!("{part_id}:{start}:{end}:{}", sha256_hex(bytes)).as_bytes()
                    )
                ),
                kind,
                quote_depth,
                part_id: part_id.to_string(),
                byte_range: [start, end],
                sha256: sha256_hex(bytes),
                truncated: false,
            }
        })
        .collect()
}

fn lines_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for segment in text.split_inclusive('\n') {
        lines.push((start, segment));
        start += segment.len();
    }
    lines
}

fn quote_depth(line: &str) -> usize {
    let mut depth = 0usize;
    let mut chars = line.trim_start().chars().peekable();

    while chars.next_if_eq(&'>').is_some() {
        depth += 1;
        while chars.next_if_eq(&' ').is_some() {}
    }

    depth
}

fn is_reply_attribution(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("-----Original Message-----") {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    (lower.starts_with("on ") && lower.ends_with("wrote:"))
        || lower.starts_with("from:") && lower.contains("sent:") && lower.contains("subject:")
}

/// Index of the line where an embedded earlier message starts, if any.
///
/// Top-posting clients (Outlook most commonly) append the original message
/// without `>` markers, so per-line depth detection never sees it. The fixed
/// patterns here are: an underscore separator followed by a `From:` line, an
/// `-----Original Message-----` divider, and a bare `From:`/`Sent:`/`Subject:`
/// header block. Everything from the matched line onward is quoted.
fn tail_quote_start(lines: &[(usize, &str)]) -> Option<usize> {
    lines.iter().enumerate().position(|(idx, (_, line))| {
        let trimmed = line.trim();
        trimmed.eq_ignore_ascii_case("-----Original Message-----")
            || (is_underscore_separator(trimmed) && next_line_is_from(lines, idx))
            || is_embedded_header_block(lines, idx)
    })
}

fn is_underscore_separator(trimmed: &str) -> bool {
    trimmed.len() >= 8 && trimmed.chars().all(|ch| ch == '_')
}

fn next_line_is_from(lines: &[(usize, &str)], idx: usize) -> bool {
    lines
        .iter()
        .skip(idx + 1)
        .take(3)
        .map(|(_, line)| line.trim())
        .find(|line| !line.is_empty())
        .is_some_and(|line| starts_with_ignore_case(line, "from:"))
}

fn is_embedded_header_block(lines: &[(usize, &str)], idx: usize) -> bool {
    if !starts_with_ignore_case(lines[idx].1.trim(), "from:") {
        return false;
    }

    let mut has_sent = false;
    let mut has_subject = false;
    for (_, line) in lines.iter().skip(idx + 1).take(5) {
        let line = line.trim();
        has_sent |=
            starts_with_ignore_case(line, "sent:") || starts_with_ignore_case(line, "date:");
        has_subject |= starts_with_ignore_case(line, "subject:");
    }
    has_sent && has_subject
}

fn starts_with_ignore_case(line: &str, prefix: &str) -> bool {
    line.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// Cheap, deterministic markup check: a closing tag (`</x`) is a strong
/// signal that a text/plain part embeds raw HTML.
fn text_looks_like_html(text: &str) -> bool {
    text.as_bytes()
        .windows(3)
        .any(|window| window[0] == b'<' && window[1] == b'/' && window[2].is_ascii_alphabetic())
}

fn utf8_boundary_at_or_before(text: &str, max: usize) -> usize {
    if max >= text.len() {
        return text.len();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn render_body_text(body: &BodyDto, quote_mode: QuoteMode) -> String {
    match quote_mode {
        QuoteMode::Keep => body.text.clone(),
        QuoteMode::Drop => body
            .fragments
            .iter()
            .filter(|fragment| fragment.kind != "quoted")
            .filter_map(|fragment| {
                body.text
                    .get(fragment.byte_range[0]..fragment.byte_range[1])
            })
            .collect::<Vec<_>>()
            .join(""),
        QuoteMode::Collapse => {
            let mut out = String::new();
            for fragment in &body.fragments {
                if fragment.kind == "quoted" {
                    let lines = body
                        .text
                        .get(fragment.byte_range[0]..fragment.byte_range[1])
                        .map(|text| text.lines().count())
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "\n[quoted content collapsed: {lines} line{}]\n",
                        if lines == 1 { "" } else { "s" }
                    ));
                } else if let Some(text) = body
                    .text
                    .get(fragment.byte_range[0]..fragment.byte_range[1])
                {
                    out.push_str(text);
                }
            }
            out
        }
    }
}

fn thread_components(
    messages: &[MessageDto],
    id_to_indexes: &HashMap<String, Vec<usize>>,
    subject_fallback: bool,
) -> Vec<Vec<usize>> {
    let mut graph = vec![BTreeSet::<usize>::new(); messages.len()];

    for (idx, message) in messages.iter().enumerate() {
        for linked_idx in linked_message_indexes(message, id_to_indexes) {
            if linked_idx != idx {
                graph[idx].insert(linked_idx);
                graph[linked_idx].insert(idx);
            }
        }
    }

    let mut by_thread_hint = HashMap::<String, Vec<usize>>::new();
    for (idx, message) in messages.iter().enumerate() {
        if let Some(root) = message
            .thread
            .root_message_id
            .as_ref()
            .or(message.thread.parent_message_id.as_ref())
        {
            by_thread_hint.entry(root.clone()).or_default().push(idx);
        }
    }
    for indexes in by_thread_hint.values() {
        for window in indexes.windows(2) {
            graph[window[0]].insert(window[1]);
            graph[window[1]].insert(window[0]);
        }
    }

    if subject_fallback {
        let mut by_subject = HashMap::<String, Vec<usize>>::new();
        for (idx, message) in messages.iter().enumerate() {
            if !linked_message_indexes(message, id_to_indexes).is_empty() {
                continue;
            }
            if let Some(subject) = message
                .thread
                .base_subject
                .as_deref()
                .and_then(subject_fallback_key)
            {
                by_subject.entry(subject).or_default().push(idx);
            }
        }
        for indexes in by_subject.values() {
            for window in indexes.windows(2) {
                graph[window[0]].insert(window[1]);
                graph[window[1]].insert(window[0]);
            }
        }
    }

    let mut seen = vec![false; messages.len()];
    let mut components = Vec::new();
    for idx in 0..messages.len() {
        if seen[idx] {
            continue;
        }

        let mut stack = vec![idx];
        let mut component = Vec::new();
        seen[idx] = true;
        while let Some(current) = stack.pop() {
            component.push(current);
            for next in &graph[current] {
                if !seen[*next] {
                    seen[*next] = true;
                    stack.push(*next);
                }
            }
        }
        component.sort_by_key(|idx| message_sort_key(&messages[*idx]));
        components.push(component);
    }

    components.sort_by_key(|component| {
        component
            .first()
            .map(|idx| message_sort_key(&messages[*idx]))
            .unwrap_or_default()
    });
    components
}

fn linked_message_indexes(
    message: &MessageDto,
    id_to_indexes: &HashMap<String, Vec<usize>>,
) -> BTreeSet<usize> {
    let parent_candidates = parent_candidate_ids(message);
    message
        .thread
        .references
        .iter()
        .chain(parent_candidates.iter())
        .filter_map(|id| id_to_indexes.get(id))
        .flatten()
        .copied()
        .collect()
}

fn build_thread(
    component: Vec<usize>,
    all_messages: &[MessageDto],
    id_to_indexes: &HashMap<String, Vec<usize>>,
    duplicate_ids: &BTreeSet<String>,
) -> ThreadDto {
    let component_set = component.iter().copied().collect::<HashSet<_>>();
    let subject_link_node_indexes =
        subject_link_node_indexes(&component, all_messages, id_to_indexes);
    let id_to_node = component
        .iter()
        .filter_map(|idx| {
            let message = &all_messages[*idx];
            let message_id = message.message.message_id.as_ref()?;
            if duplicate_ids.contains(message_id) {
                return None;
            }
            Some((message_id.clone(), message.message.node_id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut missing_parent_message_ids = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut child_map = HashMap::<String, Vec<String>>::new();

    for idx in &component {
        let message = all_messages[*idx].clone();
        let parent_candidates = parent_candidate_ids(&message);
        let mut first_missing_parent = None;
        let mut parent_node_id = None;

        for parent_id in &parent_candidates {
            let parent_supplied_in_this_thread = id_to_indexes
                .get(parent_id)
                .is_some_and(|indexes| indexes.iter().any(|idx| component_set.contains(idx)));
            if let Some(node_id) = id_to_node.get(parent_id) {
                parent_node_id = Some(node_id.clone());
                break;
            }
            if !parent_supplied_in_this_thread && first_missing_parent.is_none() {
                first_missing_parent = Some(parent_id.clone());
            }
        }

        if parent_node_id.is_none()
            && let Some(parent_id) = first_missing_parent
        {
            missing_parent_message_ids.insert(parent_id);
        }

        if let Some(parent_node_id) = &parent_node_id {
            child_map
                .entry(parent_node_id.clone())
                .or_default()
                .push(message.message.node_id.clone());
        }

        let has_unresolved_parent = parent_node_id.is_none() && !parent_candidates.is_empty();
        let link_confidence = if parent_node_id.is_some() {
            "id"
        } else if has_unresolved_parent {
            "orphan"
        } else if subject_link_node_indexes.contains(idx) {
            "subject"
        } else {
            "root"
        };

        nodes.push(ThreadNodeDto {
            node_id: message.message.node_id.clone(),
            message_id: message.message.message_id.clone(),
            parent_node_id,
            child_node_ids: Vec::new(),
            depth: 0,
            link_confidence: link_confidence.to_string(),
            message,
        });
    }

    let index_by_node = nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.node_id.clone(), idx))
        .collect::<HashMap<_, _>>();

    for node in &mut nodes {
        let mut children = child_map.remove(&node.node_id).unwrap_or_default();
        children.sort();
        node.child_node_ids = children;
    }

    let depth_cache = nodes
        .iter()
        .map(|node| {
            (
                node.node_id.clone(),
                depth_for_node(node, &nodes, &index_by_node),
            )
        })
        .collect::<HashMap<_, _>>();
    for node in &mut nodes {
        node.depth = *depth_cache.get(&node.node_id).unwrap_or(&0);
    }

    nodes.sort_by_key(thread_node_sort_key);
    let timeline = nodes
        .iter()
        .map(|node| node.node_id.clone())
        .collect::<Vec<_>>();
    let root_message_id = nodes
        .iter()
        .find(|node| node.parent_node_id.is_none())
        .and_then(|node| node.message_id.clone())
        .or_else(|| nodes.first().and_then(|node| node.message_id.clone()));
    let base_subject = nodes
        .iter()
        .find_map(|node| node.message.thread.base_subject.clone());
    let participants = collect_participants(&nodes);
    let time_range = TimeRangeDto {
        start: thread_date_bound(&nodes, DateBound::Start),
        end: thread_date_bound(&nodes, DateBound::End),
    };
    let duplicate_message_ids = nodes
        .iter()
        .filter_map(|node| node.message_id.clone())
        .filter(|id| duplicate_ids.contains(id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let missing_parent_message_ids = missing_parent_message_ids.into_iter().collect::<Vec<_>>();

    let mut diagnostics = Vec::new();
    if !missing_parent_message_ids.is_empty() {
        diagnostics.push(DiagnosticDto::warning(
            "MISSING_THREAD_PARENT",
            "One or more parent Message-ID values were referenced but not supplied.",
            Some("$.threads[].missing_parent_message_ids"),
            None,
        ));
    }
    if !duplicate_message_ids.is_empty() {
        diagnostics.push(DiagnosticDto::warning(
            "DUPLICATE_MESSAGE_ID",
            "This thread contains duplicate Message-ID values.",
            Some("$.threads[].duplicate_message_ids"),
            None,
        ));
    }

    ThreadDto {
        thread_id: thread_id_for_nodes(&nodes),
        root_message_id,
        base_subject,
        participants,
        time_range,
        nodes,
        timeline,
        missing_parent_message_ids,
        duplicate_message_ids,
        diagnostics,
    }
}

fn subject_link_node_indexes(
    component: &[usize],
    messages: &[MessageDto],
    id_to_indexes: &HashMap<String, Vec<usize>>,
) -> BTreeSet<usize> {
    let mut by_subject = HashMap::<String, Vec<usize>>::new();
    for idx in component {
        if !linked_message_indexes(&messages[*idx], id_to_indexes).is_empty() {
            continue;
        }
        if let Some(subject) = messages[*idx]
            .thread
            .base_subject
            .as_deref()
            .and_then(subject_fallback_key)
        {
            by_subject.entry(subject).or_default().push(*idx);
        }
    }

    by_subject
        .into_values()
        .filter(|indexes| indexes.len() > 1)
        .flatten()
        .collect()
}

fn subject_fallback_key(subject: &str) -> Option<String> {
    let subject = subject.trim();
    if subject.is_empty() {
        return None;
    }

    Some(subject.to_ascii_lowercase())
}

fn thread_node_sort_key(node: &ThreadNodeDto) -> (usize, bool, i64, String, String) {
    let base = message_sort_key(&node.message);
    (node.depth, base.0, base.1, base.2, base.3)
}

fn thread_id_for_nodes(nodes: &[ThreadNodeDto]) -> String {
    let seed = nodes
        .iter()
        .map(|node| {
            format!(
                "{}:{}",
                node.node_id,
                node.message_id
                    .as_deref()
                    .unwrap_or(node.message.source.sha256.as_str())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("thr_{}", short_sha256(seed.as_bytes()))
}

fn depth_for_node(
    node: &ThreadNodeDto,
    nodes: &[ThreadNodeDto],
    index_by_node: &HashMap<String, usize>,
) -> usize {
    let mut depth = 0usize;
    let mut seen = HashSet::new();
    let mut parent = node.parent_node_id.as_ref();
    while let Some(parent_id) = parent {
        if !seen.insert(parent_id.clone()) {
            break;
        }
        let Some(parent_idx) = index_by_node.get(parent_id) else {
            break;
        };
        depth += 1;
        parent = nodes[*parent_idx].parent_node_id.as_ref();
    }
    depth
}

fn collect_participants(nodes: &[ThreadNodeDto]) -> Vec<AddressDto> {
    let mut seen = BTreeSet::new();
    let mut participants = Vec::new();
    for node in nodes {
        for address in node
            .message
            .message
            .from
            .iter()
            .chain(node.message.message.to.iter())
            .chain(node.message.message.cc.iter())
        {
            let key = format!(
                "{}\0{}",
                address.name.as_deref().unwrap_or_default(),
                address.email.as_deref().unwrap_or_default()
            );
            if seen.insert(key) {
                participants.push(address.clone());
            }
        }
    }
    participants
}

fn ensure_unique_node_ids(messages: &mut [MessageDto]) {
    let mut seen = HashMap::<String, usize>::new();
    for message in messages {
        let original = message.message.node_id.clone();
        let count = seen.entry(original.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            message.message.node_id = format!("{original}_{}", *count);
        }
    }
}

fn parent_candidate_ids(message: &MessageDto) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(parent_id) = &message.message.in_reply_to {
        candidates.push(parent_id.clone());
    }
    if let Some(reference_parent) = message.message.references.last()
        && !candidates
            .iter()
            .any(|candidate| candidate == reference_parent)
    {
        candidates.push(reference_parent.clone());
    }
    candidates
}

#[derive(Clone, Copy)]
enum DateBound {
    Start,
    End,
}

fn thread_date_bound(nodes: &[ThreadNodeDto], bound: DateBound) -> Option<String> {
    let mut dates = nodes
        .iter()
        .filter_map(|node| {
            Some((
                date_sort_tuple(&node.message),
                node.message.message.date.as_ref()?.clone(),
            ))
        })
        .collect::<Vec<_>>();

    match bound {
        DateBound::Start => dates.sort_by_key(|(key, _)| *key),
        DateBound::End => dates.sort_by_key(|(key, _)| std::cmp::Reverse(*key)),
    }

    dates.into_iter().map(|(_, date)| date).next()
}

fn message_sort_key(message: &MessageDto) -> (bool, i64, String, String) {
    let (missing_date, timestamp) = date_sort_tuple(message);
    (
        missing_date,
        timestamp,
        message.source.path.clone(),
        message.source.sha256.clone(),
    )
}

fn date_sort_tuple(message: &MessageDto) -> (bool, i64) {
    match message.message.date_timestamp {
        Some(timestamp) => (false, timestamp),
        None => (true, 0),
    }
}

fn part_paths(message: &Message<'_>) -> Vec<String> {
    let mut paths = (0..message.parts.len())
        .map(|idx| (idx + 1).to_string())
        .collect::<Vec<_>>();
    assign_part_path(message, 0, "1".to_string(), &mut paths);
    paths
}

fn assign_part_path(message: &Message<'_>, idx: usize, path: String, paths: &mut [String]) {
    if let Some(slot) = paths.get_mut(idx) {
        *slot = path.clone();
    }

    let Some(part) = message.parts.get(idx) else {
        return;
    };
    let Some(children) = part.sub_parts() else {
        return;
    };

    for (pos, child) in children.iter().enumerate() {
        assign_part_path(
            message,
            *child as usize,
            format!("{path}.{}", pos + 1),
            paths,
        );
    }
}

fn address_list(address: Option<&Address<'_>>) -> Vec<AddressDto> {
    match address {
        Some(Address::List(list)) => list
            .iter()
            .map(|addr| AddressDto {
                name: addr.name.as_ref().map(|name| name.to_string()),
                email: addr.address.as_ref().map(|address| address.to_string()),
            })
            .collect(),
        Some(Address::Group(groups)) => groups
            .iter()
            .flat_map(|group| group.addresses.iter())
            .map(|addr| AddressDto {
                name: addr.name.as_ref().map(|name| name.to_string()),
                email: addr.address.as_ref().map(|address| address.to_string()),
            })
            .collect(),
        None => Vec::new(),
    }
}

fn ids_from_header(header: &HeaderValue<'_>) -> Vec<String> {
    match header {
        HeaderValue::Text(text) => normalize_message_id(text).into_iter().collect(),
        HeaderValue::TextList(list) => list
            .iter()
            .filter_map(|text| normalize_message_id(text))
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_message_id(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(value)
        .trim();

    (!value.is_empty()).then(|| value.to_string())
}

fn header_value_to_string(value: &HeaderValue<'_>) -> String {
    match value {
        HeaderValue::Address(address) => render_addresses(&address_list(Some(address))),
        HeaderValue::Text(text) => text.to_string(),
        HeaderValue::TextList(list) => list
            .iter()
            .map(|text| text.as_ref())
            .collect::<Vec<_>>()
            .join(" "),
        HeaderValue::DateTime(date) => date.to_rfc3339(),
        HeaderValue::ContentType(content_type) => content_type_name(content_type),
        HeaderValue::Received(received) => format!("{received:?}"),
        HeaderValue::Empty => String::new(),
    }
}

fn render_addresses(addresses: &[AddressDto]) -> String {
    addresses
        .iter()
        .map(|address| match (&address.name, &address.email) {
            (Some(name), Some(email)) => format!("{name} <{email}>"),
            (None, Some(email)) => email.clone(),
            (Some(name), None) => name.clone(),
            (None, None) => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn raw_header_value(raw: &str, name: &str) -> String {
    raw.strip_prefix(name)
        .and_then(|value| value.strip_prefix(':'))
        .unwrap_or(raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn safe_filename(filename: &str) -> String {
    let sanitized = filename
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '\0') {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();

    let sanitized = sanitized.trim().trim_matches('.').to_string();
    if sanitized.is_empty() {
        "attachment".to_string()
    } else {
        sanitized
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn short_sha256(bytes: &[u8]) -> String {
    let hash = sha256_hex(bytes);
    hash[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &[u8] = br#"Message-ID: <root@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Project update
Content-Type: text/plain; charset=utf-8

Fresh line.

On Tue, Bob wrote:
> quoted line
"#;

    const ATTACHMENT: &[u8] = b"Message-ID: <attach@example.com>\r\nDate: Wed, 27 May 2026 10:00:00 +0000\r\nFrom: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Attachment\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSee attached.\r\n--b\r\nContent-Type: text/plain; name=\"note.txt\"\r\nContent-Disposition: attachment; filename=\"note.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8K\r\n--b--\r\n";

    #[test]
    fn parses_message_with_fresh_and_quoted_fragments() {
        let dto = parse_message_bytes("simple.eml", SIMPLE, RenderOptions::default()).unwrap();
        assert_eq!(dto.message.message_id.as_deref(), Some("root@example.com"));
        assert!(dto.body.text.contains("Fresh line."));
        assert!(dto.body.fragments.iter().any(|f| f.kind == "fresh"));
        assert!(dto.body.fragments.iter().any(|f| f.kind == "quoted"));
        assert!(dto.body.fragments.iter().any(|p| p.part_id == "1"));
    }

    #[test]
    fn renders_quotes_collapsed() {
        let dto = parse_message_bytes("simple.eml", SIMPLE, RenderOptions::default()).unwrap();
        let text = render_message_text(&dto, QuoteMode::Collapse);
        assert!(text.contains("Fresh line."));
        assert!(text.contains("quoted content collapsed"));
    }

    #[test]
    fn text_rendering_does_not_list_body_parts_as_attachments() {
        let dto = parse_message_bytes("simple.eml", SIMPLE, RenderOptions::default()).unwrap();
        let text = render_message_text(&dto, QuoteMode::Keep);
        assert!(!text.contains("Attachments and parts:"));
    }

    #[test]
    fn builds_thread_timeline() {
        let reply = br#"Message-ID: <reply@example.com>
In-Reply-To: <root@example.com>
References: <root@example.com>
Date: Wed, 27 May 2026 11:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Project update
Content-Type: text/plain; charset=utf-8

Reply text.
"#;
        let root = parse_message_bytes("root.eml", SIMPLE, RenderOptions::default()).unwrap();
        let reply = parse_message_bytes("reply.eml", reply, RenderOptions::default()).unwrap();
        let envelope = build_thread_envelope(vec![reply, root], false);
        assert_eq!(envelope.threads.len(), 1);
        assert_eq!(envelope.threads[0].timeline.len(), 2);
        assert_eq!(envelope.threads[0].nodes.len(), 2);
    }

    #[test]
    fn groups_in_reply_to_without_references() {
        let reply = br#"Message-ID: <reply@example.com>
In-Reply-To: <root@example.com>
Date: Wed, 27 May 2026 11:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Project update
Content-Type: text/plain; charset=utf-8

Reply text.
"#;
        let root = parse_message_bytes("root.eml", SIMPLE, RenderOptions::default()).unwrap();
        let reply = parse_message_bytes("reply.eml", reply, RenderOptions::default()).unwrap();
        let envelope = build_thread_envelope(vec![reply, root], false);
        assert_eq!(envelope.threads.len(), 1);
        assert_eq!(envelope.threads[0].nodes.len(), 2);
        assert!(
            envelope.threads[0]
                .nodes
                .iter()
                .any(|node| node.parent_node_id.is_some())
        );
    }

    #[test]
    fn duplicate_message_ids_have_unique_node_ids() {
        let first = parse_message_bytes("first.eml", SIMPLE, RenderOptions::default()).unwrap();
        let second = parse_message_bytes("second.eml", SIMPLE, RenderOptions::default()).unwrap();
        assert_eq!(first.message.node_id, second.message.node_id);

        let envelope = build_thread_envelope(vec![first, second], false);
        let node_ids = envelope
            .threads
            .iter()
            .flat_map(|thread| thread.nodes.iter().map(|node| node.node_id.clone()))
            .collect::<Vec<_>>();
        let unique_node_ids = node_ids.iter().collect::<BTreeSet<_>>();
        assert_eq!(node_ids.len(), unique_node_ids.len());
        let thread_ids = envelope
            .threads
            .iter()
            .map(|thread| thread.thread_id.clone())
            .collect::<Vec<_>>();
        let unique_thread_ids = thread_ids.iter().collect::<BTreeSet<_>>();
        assert_eq!(thread_ids.len(), unique_thread_ids.len());
        assert!(
            envelope
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "DUPLICATE_MESSAGE_ID")
        );
        assert!(
            envelope
                .threads
                .iter()
                .any(|thread| thread.duplicate_message_ids == ["root@example.com"])
        );
    }

    #[test]
    fn timezone_offsets_are_sorted_by_instant() {
        let later_local = br#"Message-ID: <later-local@example.com>
Date: Wed, 27 May 2026 09:30:00 -0700
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Same subject
Content-Type: text/plain; charset=utf-8

Later by UTC.
"#;
        let earlier_utc = br#"Message-ID: <earlier-utc@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Same subject
Content-Type: text/plain; charset=utf-8

Earlier by UTC.
"#;
        let later =
            parse_message_bytes("later.eml", later_local, RenderOptions::default()).unwrap();
        let earlier =
            parse_message_bytes("earlier.eml", earlier_utc, RenderOptions::default()).unwrap();
        let envelope = build_thread_envelope(vec![later, earlier], true);
        assert_eq!(envelope.threads.len(), 1);
        assert_eq!(
            envelope.threads[0].nodes[0]
                .message
                .message
                .message_id
                .as_deref(),
            Some("earlier-utc@example.com")
        );
        assert_eq!(
            envelope.threads[0].time_range.start.as_deref(),
            Some("2026-05-27T10:00:00Z")
        );
        assert_eq!(
            envelope.threads[0].time_range.end.as_deref(),
            Some("2026-05-27T16:30:00Z")
        );
    }

    #[test]
    fn falls_back_to_references_when_in_reply_to_is_unresolved() {
        let reply = br#"Message-ID: <reply@example.com>
In-Reply-To: <missing@example.com>
References: <root@example.com>
Date: Wed, 27 May 2026 11:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Project update
Content-Type: text/plain; charset=utf-8

Reply text.
"#;
        let root = parse_message_bytes("root.eml", SIMPLE, RenderOptions::default()).unwrap();
        let reply = parse_message_bytes("reply.eml", reply, RenderOptions::default()).unwrap();
        let envelope = build_thread_envelope(vec![reply, root], false);
        let thread = &envelope.threads[0];
        let root_node = thread
            .nodes
            .iter()
            .find(|node| node.message_id.as_deref() == Some("root@example.com"))
            .unwrap();
        let reply_node = thread
            .nodes
            .iter()
            .find(|node| node.message_id.as_deref() == Some("reply@example.com"))
            .unwrap();
        assert_eq!(reply_node.parent_node_id.as_ref(), Some(&root_node.node_id));
        assert!(thread.missing_parent_message_ids.is_empty());
    }

    #[test]
    fn includes_single_part_root_attachment_metadata() {
        let raw = b"Message-ID: <root-attachment@example.com>\r\nDate: Wed, 27 May 2026 10:00:00 +0000\r\nFrom: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Attachment root\r\nContent-Type: text/plain; name=\"note.txt\"\r\nContent-Disposition: attachment; filename=\"note.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8K\r\n";
        let dto =
            parse_message_bytes("root-attachment.eml", raw, RenderOptions::default()).unwrap();
        assert_eq!(dto.parts.len(), 1);
        assert_eq!(dto.parts[0].part_id, "1");
        assert_eq!(dto.parts[0].kind, "attachment");
        assert_eq!(extract_part_bytes(raw, "1").unwrap(), b"Hello\n");
    }

    #[test]
    fn timeline_orders_parent_before_earlier_dated_child() {
        let root = br#"Message-ID: <late-root@example.com>
Date: Wed, 27 May 2026 12:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Late root
Content-Type: text/plain; charset=utf-8

Root text.
"#;
        let reply = br#"Message-ID: <early-reply@example.com>
In-Reply-To: <late-root@example.com>
Date: Wed, 27 May 2026 09:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Late root
Content-Type: text/plain; charset=utf-8

Reply text.
"#;
        let root = parse_message_bytes("root.eml", root, RenderOptions::default()).unwrap();
        let reply = parse_message_bytes("reply.eml", reply, RenderOptions::default()).unwrap();
        let envelope = build_thread_envelope(vec![reply, root], false);
        let nodes = &envelope.threads[0].nodes;
        assert_eq!(
            nodes[0].message.message.message_id.as_deref(),
            Some("late-root@example.com")
        );
        assert_eq!(
            nodes[1].message.message.message_id.as_deref(),
            Some("early-reply@example.com")
        );
    }

    #[test]
    fn subject_fallback_groups_unlinked_messages_with_ids() {
        let first = br#"Message-ID: <first@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Same subject
Content-Type: text/plain; charset=utf-8

First.
"#;
        let second = br#"Message-ID: <second@example.com>
Date: Wed, 27 May 2026 11:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Same subject
Content-Type: text/plain; charset=utf-8

Second.
"#;
        let first = parse_message_bytes("first.eml", first, RenderOptions::default()).unwrap();
        let second = parse_message_bytes("second.eml", second, RenderOptions::default()).unwrap();
        assert_eq!(
            build_thread_envelope(vec![first.clone(), second.clone()], false)
                .threads
                .len(),
            2
        );
        assert_eq!(
            build_thread_envelope(vec![first, second], true)
                .threads
                .len(),
            1
        );
    }

    #[test]
    fn subject_fallback_ignores_empty_subjects() {
        let first = br#"Message-ID: <first-empty-subject@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Content-Type: text/plain; charset=utf-8

First.
"#;
        let second = br#"Message-ID: <second-empty-subject@example.com>
Date: Wed, 27 May 2026 11:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Content-Type: text/plain; charset=utf-8

Second.
"#;
        let first = parse_message_bytes("first.eml", first, RenderOptions::default()).unwrap();
        let second = parse_message_bytes("second.eml", second, RenderOptions::default()).unwrap();
        assert_eq!(
            build_thread_envelope(vec![first, second], true)
                .threads
                .len(),
            2
        );
    }

    #[test]
    fn groups_replies_with_shared_missing_root_reference() {
        let first = br#"Message-ID: <first-reply@example.com>
References: <missing-root@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Re: Shared root
Content-Type: text/plain; charset=utf-8

First reply.
"#;
        let second = br#"Message-ID: <second-reply@example.com>
References: <missing-root@example.com>
Date: Wed, 27 May 2026 11:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Shared root
Content-Type: text/plain; charset=utf-8

Second reply.
"#;
        let first = parse_message_bytes("first.eml", first, RenderOptions::default()).unwrap();
        let second = parse_message_bytes("second.eml", second, RenderOptions::default()).unwrap();
        let envelope = build_thread_envelope(vec![first, second], false);
        assert_eq!(envelope.threads.len(), 1);
        assert_eq!(
            envelope.threads[0].missing_parent_message_ids,
            ["missing-root@example.com"]
        );
        assert!(
            envelope.threads[0]
                .nodes
                .iter()
                .all(|node| node.link_confidence == "orphan")
        );
    }

    #[test]
    fn extracts_attachment_by_mime_path_id() {
        let dto = parse_message_bytes("attach.eml", ATTACHMENT, RenderOptions::default()).unwrap();
        let attachment = dto
            .parts
            .iter()
            .find(|part| part.kind == "attachment")
            .unwrap();
        assert_eq!(attachment.part_id, "1.2");
        assert_eq!(attachment.filename.as_deref(), Some("note.txt"));
        assert_eq!(extract_part_bytes(ATTACHMENT, "1.2").unwrap(), b"Hello\n");
    }

    #[test]
    fn extracts_nested_rfc822_message_bytes() {
        let raw = b"Message-ID: <outer@example.com>\r\nDate: Wed, 27 May 2026 10:00:00 +0000\r\nFrom: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Forwarded message\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nForwarded below.\r\n--b\r\nContent-Type: message/rfc822\r\nContent-Disposition: attachment; filename=\"forwarded.eml\"\r\n\r\nMessage-ID: <nested@example.com>\r\nDate: Wed, 27 May 2026 09:00:00 +0000\r\nFrom: Carol <carol@example.com>\r\nTo: Alice <alice@example.com>\r\nSubject: Nested\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nNested body.\r\n--b--\r\n";
        let dto = parse_message_bytes("outer.eml", raw, RenderOptions::default()).unwrap();
        let nested = dto
            .parts
            .iter()
            .find(|part| part.content_type == "message/rfc822")
            .unwrap();
        assert_eq!(nested.part_id, "1.2");
        assert_eq!(nested.kind, "attachment");
        assert!(nested.extractable);
        assert!(nested.decoded_size_bytes > 0);
        let extracted = extract_part_bytes(raw, "1.2").unwrap();
        assert!(String::from_utf8_lossy(&extracted).contains("Message-ID: <nested@example.com>"));
        assert_eq!(
            dto.nested_messages[0].message_id.as_deref(),
            Some("nested@example.com")
        );
    }

    #[test]
    fn html_only_bodies_report_source_and_alternatives() {
        let raw = br#"Message-ID: <html-only@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: HTML only
Content-Type: text/html; charset=utf-8

<html><body><p>Hello <b>Bob</b></p></body></html>
"#;
        let dto = parse_message_bytes("html.eml", raw, RenderOptions::default()).unwrap();
        assert!(dto.body.html_available);
        assert_eq!(dto.body.text_source, "html");
        assert!(dto.body.html.is_none());
        assert!(
            dto.body
                .alternatives
                .iter()
                .any(|alternative| alternative.kind == "html")
        );
        assert!(
            dto.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "HTML_CONVERTED_TO_TEXT")
        );
    }

    #[test]
    fn records_body_truncation() {
        let options = RenderOptions {
            max_body_bytes: 12,
            ..RenderOptions::default()
        };
        let dto = parse_message_bytes("simple.eml", SIMPLE, options).unwrap();
        assert!(dto.body.truncation.truncated);
        assert!(
            dto.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "BODY_TRUNCATED")
        );
        for fragment in dto
            .body
            .fragments
            .iter()
            .filter(|fragment| fragment.truncated)
        {
            let bytes = &dto.body.text.as_bytes()[fragment.byte_range[0]..fragment.byte_range[1]];
            assert_eq!(fragment.sha256, sha256_hex(bytes));
        }
    }

    const OUTLOOK_REPLY: &[u8] = br#"Message-ID: <outlook@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: Re: Project update
Content-Type: text/plain; charset=utf-8

Sounds good, see answers below.

________________________________
From: Alice <alice@example.com>
Sent: Wednesday, 27 May 2026 09:00:00
To: Bob <bob@example.com>
Subject: Project update

Original message text that carries no quote markers.
"#;

    #[test]
    fn outlook_separator_marks_tail_as_quoted() {
        let dto =
            parse_message_bytes("outlook.eml", OUTLOOK_REPLY, RenderOptions::default()).unwrap();
        let quoted = dto
            .body
            .fragments
            .iter()
            .filter(|fragment| fragment.kind == "quoted")
            .collect::<Vec<_>>();
        assert_eq!(quoted.len(), 1);
        assert_eq!(quoted[0].byte_range[1], dto.body.text.len());
        let quoted_text = &dto.body.text[quoted[0].byte_range[0]..quoted[0].byte_range[1]];
        assert!(quoted_text.starts_with("________________________________"));
        assert!(quoted_text.contains("Original message text"));

        let rendered = render_message_text(&dto, QuoteMode::Drop);
        assert!(rendered.contains("Sounds good"));
        assert!(!rendered.contains("Original message text"));
    }

    #[test]
    fn original_message_divider_marks_tail_as_quoted() {
        let raw = br#"Message-ID: <divider@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: RE: Project update
Content-Type: text/plain; charset=utf-8

Agreed.

-----Original Message-----
From: Alice <alice@example.com>

Earlier text without markers.
"#;
        let dto = parse_message_bytes("divider.eml", raw, RenderOptions::default()).unwrap();
        let rendered = render_message_text(&dto, QuoteMode::Drop);
        assert!(rendered.contains("Agreed."));
        assert!(!rendered.contains("Earlier text without markers."));
    }

    #[test]
    fn embedded_header_block_marks_tail_as_quoted() {
        let raw = br#"Message-ID: <block@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Bob <bob@example.com>
To: Alice <alice@example.com>
Subject: RE: Project update
Content-Type: text/plain; charset=utf-8

Works for me.

From: Alice <alice@example.com>
Sent: Wednesday, 27 May 2026 09:00:00
To: Bob <bob@example.com>
Subject: Project update

Earlier text without markers.
"#;
        let dto = parse_message_bytes("block.eml", raw, RenderOptions::default()).unwrap();
        let rendered = render_message_text(&dto, QuoteMode::Drop);
        assert!(rendered.contains("Works for me."));
        assert!(!rendered.contains("Earlier text without markers."));
    }

    #[test]
    fn collapse_marker_counts_lines() {
        let dto = parse_message_bytes("simple.eml", SIMPLE, RenderOptions::default()).unwrap();
        let rendered = render_message_text(&dto, QuoteMode::Collapse);
        assert!(rendered.contains("[quoted content collapsed: 2 lines]"));
    }

    #[test]
    fn text_body_with_markup_gets_diagnostic() {
        let raw = br#"Message-ID: <markup@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Markup
Content-Type: text/plain; charset=utf-8

Hi Bob,

<p style="font-family: Arial;">Rate us!</p><a href="https://example.com">link</a>
"#;
        let dto = parse_message_bytes("markup.eml", raw, RenderOptions::default()).unwrap();
        assert!(
            dto.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TEXT_BODY_CONTAINS_HTML")
        );
    }

    #[test]
    fn clean_text_body_gets_no_markup_diagnostic() {
        let dto = parse_message_bytes("simple.eml", SIMPLE, RenderOptions::default()).unwrap();
        assert!(
            dto.diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "TEXT_BODY_CONTAINS_HTML")
        );
        // Math with angle brackets is not markup.
        assert!(!text_looks_like_html("a<b and c>d hold, even a </ alone"));
        assert!(text_looks_like_html("closing </p> tag"));
    }

    #[test]
    fn body_source_alternative_is_deduplicated() {
        let dto = parse_message_bytes("simple.eml", SIMPLE, RenderOptions::default()).unwrap();
        let alternative = &dto.body.alternatives[0];
        assert_eq!(alternative.same_as.as_deref(), Some("body.text"));
        assert!(alternative.text.is_none());
        assert!(alternative.decoded_size_bytes > 0);
        assert!(!alternative.decoded_sha256.is_empty());
        assert_eq!(dto.body.text_part_id.as_deref(), Some("1"));
    }

    #[test]
    fn html_alternative_dedups_against_body_html() {
        let raw = b"Message-ID: <alt@example.com>\r\nDate: Wed, 27 May 2026 10:00:00 +0000\r\nFrom: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Alternative\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain body.\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>HTML body.</p>\r\n--b--\r\n";
        let options = RenderOptions {
            include_html: true,
            ..RenderOptions::default()
        };
        let dto = parse_message_bytes("alt.eml", raw, options).unwrap();
        assert!(dto.body.html.is_some());
        let html_alternative = dto
            .body
            .alternatives
            .iter()
            .find(|alternative| alternative.kind == "html")
            .unwrap();
        assert_eq!(html_alternative.same_as.as_deref(), Some("body.html"));
        assert!(html_alternative.html.is_none());
        let text_alternative = dto
            .body
            .alternatives
            .iter()
            .find(|alternative| alternative.kind == "text")
            .unwrap();
        assert_eq!(text_alternative.same_as.as_deref(), Some("body.text"));
        assert!(text_alternative.text.is_none());
    }

    #[test]
    fn html_alternative_dedups_even_when_truncated() {
        let raw = b"Message-ID: <alt-trunc@example.com>\r\nDate: Wed, 27 May 2026 10:00:00 +0000\r\nFrom: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Alternative\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain body.\r\n--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>HTML body that is long enough to be truncated by the byte limit.</p>\r\n--b--\r\n";
        let options = RenderOptions {
            include_html: true,
            max_body_bytes: 24,
            ..RenderOptions::default()
        };
        let dto = parse_message_bytes("alt-trunc.eml", raw, options).unwrap();
        let body_html = dto.body.html.as_deref().unwrap();
        assert!(body_html.contains("truncated by the byte limit"));
        let html_alternative = dto
            .body
            .alternatives
            .iter()
            .find(|alternative| alternative.kind == "html")
            .unwrap();
        assert_eq!(html_alternative.same_as.as_deref(), Some("body.html"));
        assert!(html_alternative.html.is_none());
        assert!(!html_alternative.truncated);
    }

    #[test]
    fn standard_headers_omit_transport_noise() {
        let raw =
            br#"Received: from mail.example.com by mx.example.com; Wed, 27 May 2026 10:00:01 +0000
DKIM-Signature: v=1; a=rsa-sha256; d=example.com; s=default; b=abc123
X-Custom-Tracker: opaque
Message-ID: <headers@example.com>
Date: Wed, 27 May 2026 10:00:00 +0000
From: Alice <alice@example.com>
To: Bob <bob@example.com>
Subject: Headers
Content-Type: text/plain; charset=utf-8

Body.
"#;
        let dto = parse_message_bytes("headers.eml", raw, RenderOptions::default()).unwrap();
        assert!(
            dto.headers
                .iter()
                .all(|header| !matches!(header.name.as_str(), "Received" | "DKIM-Signature"))
        );
        assert!(dto.headers.iter().any(|header| header.name == "Subject"));
        assert_eq!(dto.headers_omitted, 3);

        let all = parse_message_bytes(
            "headers.eml",
            raw,
            RenderOptions {
                headers: HeaderScope::All,
                ..RenderOptions::default()
            },
        )
        .unwrap();
        assert!(all.headers.iter().any(|header| header.name == "Received"));
        assert!(
            all.headers
                .iter()
                .any(|header| header.name == "X-Custom-Tracker")
        );
        assert_eq!(all.headers_omitted, 0);
    }

    #[test]
    fn quote_depth_counts_spaced_markers() {
        let dto = parse_message_bytes(
            "spaced.eml",
            br#"Message-ID: <spaced@example.com>
Subject: Spaced
Content-Type: text/plain; charset=utf-8

> > nested quote
"#,
            RenderOptions::default(),
        )
        .unwrap();
        assert!(
            dto.body
                .fragments
                .iter()
                .any(|fragment| fragment.kind == "quoted" && fragment.quote_depth == 2)
        );
    }
}
