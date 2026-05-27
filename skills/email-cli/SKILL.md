---
name: email-cli
description: Use when Codex needs to inspect, parse, summarize, thread, quote-render, or extract data from .eml files, RFC 822/RFC 5322 raw email, MIME messages, email attachments, nested message/rfc822 parts, headers, bodies, or email conversations. Prefer this skill for turning email files into LLM-friendly JSON/text, using jq over email output, reconstructing supplied message threads, or safely extracting decoded MIME parts.
---

# email-cli

Use `email-cli` as the deterministic decoder before reasoning about email
content. It turns `.eml` / RFC 822 messages into stable JSON or prompt-ready
text so Codex does not have to infer structure from raw MIME syntax.

## First Checks

1. Prefer `email-cli` on `PATH`.
2. If the current repository is the `email-cli` Rust project and the binary is
   not installed, use `cargo run --` in that repo.
3. If neither is available, say that `email-cli` is unavailable instead of
   hand-rolling MIME parsing unless the user explicitly asks for a fallback.

Use `rg --files -g '*.eml'` to find local email files when the user gives a
directory or vague location.

## Core Commands

Parse one message as JSON:

```sh
email-cli message.eml
```

Parse one message from stdin:

```sh
email-cli - < message.eml
```

Render prompt-ready text:

```sh
email-cli message.eml --format text --quotes collapse
```

Reconstruct conversations from explicitly supplied files:

```sh
email-cli thread *.eml --format json
```

Emit flat records for scripts:

```sh
email-cli messages *.eml --format ndjson
```

Extract a decoded MIME part:

```sh
email-cli extract message.eml --part 1.2 -o attachment.bin
```

## Workflow

1. Start with JSON for analysis, automation, or citation. Use text output only
   when preparing prompt context.
2. Read `diagnostics` before trusting completeness. Batch commands keep going
   after per-file failures and report them in the output.
3. Use `message.date` for normalized UTC time and `message.date_original` when
   the original header spelling or offset matters.
4. Use `body.text` as the easy path, and `body.fragments` when quote provenance
   matters.
5. Use `parts[].part_id` for extraction. Never use filenames as identifiers;
   filenames may be duplicated, absent, or unsafe.
6. Use `thread` when the user asks about a conversation. It only threads files
   explicitly supplied to the command.

## Quote And HTML Choices

- `--quotes keep`: preserve quoted content.
- `--quotes collapse`: keep prompt shape while compressing quoted runs.
- `--quotes drop`: remove quoted text from rendered text; JSON still preserves
  fragment metadata.
- Add `--html` only when raw HTML is needed. Without it, JSON still reports
  `html_available` and body alternative metadata.

## Threading Guidance

Use `email-cli thread FILE...` for conversations. It links by `Message-ID`,
`In-Reply-To`, and `References`, records missing parents and duplicates as
diagnostics, and marks unresolved replies as `link_confidence: "orphan"`.

Use `--subject-fallback` only when the user wants looser grouping. Treat
subject-only links as lower confidence.

Attached `message/rfc822` parts are extractable messages, not automatic thread
members. Extract them first if the user wants to analyze forwarded messages.

## Attachment Safety

Default JSON includes attachment metadata, not attachment contents. Extract
attachments only when needed for the task, write them to an explicit output or a
temporary path, and do not execute extracted files. Inspect extracted content
with appropriate safe tools for the file type.

## Useful jq Patterns

Message summary fields:

```sh
email-cli message.eml | jq '{subject: .message.subject, from: .message.from, date: .message.date}'
```

Attachment list:

```sh
email-cli message.eml | jq '.parts[] | select(.kind == "attachment")'
```

Thread timeline:

```sh
email-cli thread *.eml | jq '.threads[] | {subject: .base_subject, timeline: .timeline}'
```
