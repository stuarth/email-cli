use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
};

use anyhow::{Context, Result, anyhow};
use mail_parser::{
    Address, ContentType, HeaderName, HeaderValue, Message, MessageParser, MessagePart, MimeHeaders,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: &str = "1.0";
pub const DEFAULT_MAX_BODY_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuoteMode {
    Keep,
    Collapse,
    Drop,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub include_html: bool,
    pub max_body_bytes: usize,
    pub quotes: QuoteMode,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            include_html: false,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            quotes: QuoteMode::Keep,
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
    pub fragments: Vec<BodyFragmentDto>,
    pub truncation: TruncationDto,
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
    fn info(code: &str, message: impl Into<String>, json_path: Option<&str>) -> Self {
        Self {
            code: code.to_string(),
            severity: "info".to_string(),
            message: message.into(),
            json_path: json_path.map(str::to_string),
            location: None,
        }
    }

    fn warning(
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
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageDto {
    pub schema_version: String,
    pub source: SourceDto,
    pub message: MessageFieldsDto,
    pub thread: ThreadHintDto,
    pub body: BodyDto,
    pub headers: Vec<HeaderDto>,
    pub parts: Vec<PartDto>,
    pub nested_messages: Vec<NestedMessageDto>,
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

    if !message.parts.is_empty() {
        out.push_str("\n\nAttachments and parts:\n");
        for part in &message.parts {
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

            if !message.parts.is_empty() {
                out.push_str("\n\nAttachments and parts:\n");
                for part in &message.parts {
                    out.push_str(&format!(
                        "- {} {} {} {} bytes sha256:{}\n",
                        part.part_id,
                        part.filename.as_deref().unwrap_or("(unnamed)"),
                        part.content_type,
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

    let message_id = message.message_id().map(str::to_string);
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
        .map(|message_id| format!("{message_id}:{}:{}", source.path, source.sha256))
        .unwrap_or_else(|| source_hash.clone());
    let node_id = format!("msg_{}", short_sha256(node_seed.as_bytes()));
    let is_reply = parent_message_id.is_some()
        || subject
            .as_deref()
            .is_some_and(|subject| subject.trim_start().to_ascii_lowercase().starts_with("re:"));

    let date = message.date().map(|date| date.to_rfc3339());
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
    let (text, fragments, truncation) = build_body_text(
        &full_text,
        &text_part_id,
        options.max_body_bytes,
        &mut diagnostics,
    );
    let html = if options.include_html {
        message.body_html(0).map(|html| html.into_owned())
    } else {
        None
    };

    let headers = message
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
                message_id: nested.message_id().map(str::to_string),
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
            fragments,
            truncation,
        },
        headers,
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

    for (start, line) in lines_with_offsets(text) {
        let end = start + line.len();
        let depth = quote_depth(line);
        let quoted = depth > 0 || is_reply_attribution(line);
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
    if start < text.len() {
        lines.push((start, &text[start..]));
    }
    lines
}

fn quote_depth(line: &str) -> usize {
    let trimmed = line.trim_start();
    trimmed.chars().take_while(|ch| *ch == '>').count()
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
                    out.push_str(&format!(
                        "\n[quoted content collapsed: {} bytes]\n",
                        fragment.byte_range[1].saturating_sub(fragment.byte_range[0])
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
            if let Some(subject) = &message.thread.base_subject {
                by_subject
                    .entry(subject.to_ascii_lowercase())
                    .or_default()
                    .push(idx);
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
    message
        .thread
        .references
        .iter()
        .chain(message.thread.parent_message_id.iter())
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
        let parent_node_id = message
            .thread
            .parent_message_id
            .as_ref()
            .and_then(|parent_id| {
                let parent_supplied_in_this_thread = id_to_indexes
                    .get(parent_id)
                    .is_some_and(|indexes| indexes.iter().any(|idx| component_set.contains(idx)));
                match id_to_node.get(parent_id) {
                    Some(node_id) => Some(node_id.clone()),
                    None => {
                        if !parent_supplied_in_this_thread {
                            missing_parent_message_ids.insert(parent_id.clone());
                        }
                        None
                    }
                }
            });

        if let Some(parent_node_id) = &parent_node_id {
            child_map
                .entry(parent_node_id.clone())
                .or_default()
                .push(message.message.node_id.clone());
        }

        let link_confidence = if parent_node_id.is_some() {
            "id"
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
        start: nodes
            .iter()
            .filter_map(|n| n.message.message.date.clone())
            .min(),
        end: nodes
            .iter()
            .filter_map(|n| n.message.message.date.clone())
            .max(),
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
        if let Some(subject) = &messages[*idx].thread.base_subject {
            by_subject
                .entry(subject.to_ascii_lowercase())
                .or_default()
                .push(*idx);
        }
    }

    by_subject
        .into_values()
        .filter(|indexes| indexes.len() > 1)
        .flatten()
        .collect()
}

fn thread_node_sort_key(node: &ThreadNodeDto) -> (usize, String, String, String) {
    let base = message_sort_key(&node.message);
    (node.depth, base.0, base.1, base.2)
}

fn thread_id_for_nodes(nodes: &[ThreadNodeDto]) -> String {
    let seed = nodes
        .iter()
        .map(|node| {
            node.message_id
                .as_deref()
                .unwrap_or(node.message.source.sha256.as_str())
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

fn message_sort_key(message: &MessageDto) -> (String, String, String) {
    (
        message.message.date.clone().unwrap_or_default(),
        message.source.path.clone(),
        message.source.sha256.clone(),
    )
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
        HeaderValue::Text(text) => vec![text.to_string()],
        HeaderValue::TextList(list) => list.iter().map(|text| text.to_string()).collect(),
        _ => Vec::new(),
    }
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
        assert_ne!(first.message.node_id, second.message.node_id);

        let envelope = build_thread_envelope(vec![first, second], false);
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
}
