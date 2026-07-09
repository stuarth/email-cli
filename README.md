# email-cli

**The best email tool for LLMs.**

`email-cli` turns `.eml` files and RFC 822 / RFC 5322 messages into stable,
agent-friendly JSON and prompt-ready text. It decodes the email machinery that
LLMs should not have to reason about: MIME boundaries, encoded headers,
charsets, quoted-printable bodies, base64 attachments, nested messages, reply
chains, and quoted-reply clutter.

It is deterministic, read-only, and built on top of
[`stalwartlabs/mail-parser`](https://github.com/stalwartlabs/mail-parser).

## Why This Exists

Raw email is hostile input for LLMs. A single message can contain multiple body
alternatives, malformed headers, inline images, forwarded `.eml` attachments,
quoted replies, duplicated headers, suspicious filenames, and enough MIME syntax
to bury the actual conversation.

`email-cli` gives agents a cleaner contract:

- normalized message fields with source provenance
- decoded body text plus body-fragment metadata
- explicit quote handling for prompt rendering
- attachment metadata without inlining large binary payloads
- stable MIME-path part IDs for extraction
- reconstructed threads from supplied `.eml` files
- structured diagnostics instead of brittle stderr parsing
- JSON that works naturally with `jq`

It does not summarize, classify, redact, send, mutate, index, or fetch mail. It
turns email into dependable data so another tool, script, or model can do the
next step.

## Install

From this repository:

```sh
cargo install --path .
```

For development:

```sh
cargo run -- message.eml
```

## Quick Start

Parse one message as JSON:

```sh
email-cli message.eml
```

Render a message as prompt-ready text:

```sh
email-cli message.eml --format text --quotes collapse
```

Read from stdin:

```sh
cat message.eml | email-cli
email-cli - < message.eml
```

When run with no arguments from an interactive terminal, `email-cli` prints help
instead of waiting for stdin.

## Commands

```sh
email-cli [FILE|-] \
  [--format json|text] \
  [--html] \
  [--max-body-bytes N] \
  [--quotes keep|collapse|drop] \
  [--headers standard|all]

email-cli thread FILE... \
  [--format json|text] \
  [--html] \
  [--max-body-bytes N] \
  [--quotes keep|collapse|drop] \
  [--headers standard|all] \
  [--subject-fallback]

email-cli messages FILE... \
  [--format json|ndjson] \
  [--html] \
  [--max-body-bytes N] \
  [--quotes keep|collapse|drop] \
  [--headers standard|all]

email-cli extract FILE --part <MIME_PATH_ID> [-o OUT]
```

## JSON For Agents

The default output is JSON with a project-owned schema, not a direct dump of
`mail-parser` internals. Every message includes:

- `schema_version`
- `source` path, byte size, and SHA-256
- normalized message fields
- threading hints
- decoded body text
- body alternatives and HTML availability metadata
- fresh/quoted body fragments
- ordered headers, including raw header text
- MIME parts and attachment metadata
- nested `message/rfc822` summaries
- structured diagnostics

Example:

```sh
email-cli message.eml | jq '.message.subject, .body.text, .parts'
```

Output is token-lean by default. Headers are limited to a standard
identity/threading/content set, with the omitted count recorded in
`headers_omitted`; pass `--headers all` to include everything (Received chains,
DKIM/ARC signatures, `X-*` headers). The body alternative that produced
`body.text` (or `body.html` with `--html`) is not repeated inside
`body.alternatives`: its entry keeps the part metadata and hash, sets its
content field to `null`, and marks `same_as: "body.text"`. `body.text_part_id`
names the MIME part the body text came from.

Message IDs are normalized without surrounding angle brackets. Dates are
normalized to UTC RFC3339 in `message.date`, while the original Date header is
kept in `message.date_original`.

A complete single-message output, for a multipart message with a text+HTML
body and a PDF attachment (hashes and repeated entries trimmed for brevity):

```json
{
  "schema_version": "1.1",
  "source": {
    "path": "message.eml",
    "size_bytes": 1104,
    "sha256": "d5f9ba56cc…"
  },
  "message": {
    "node_id": "msg_3bd5876c4adc0c4d",
    "message_id": "root-123@example.com",
    "date": "2026-05-27T10:00:00Z",
    "date_original": "Wed, 27 May 2026 03:00:00 -0700",
    "subject": "Q3 plan — draft",
    "from": [{ "name": "Alice Example", "email": "alice@example.com" }],
    "to": [{ "name": "Bob Example", "email": "bob@example.com" }],
    "cc": [],
    "bcc": [],
    "reply_to": [],
    "in_reply_to": null,
    "references": []
  },
  "thread": {
    "parent_message_id": null,
    "root_message_id": null,
    "references": [],
    "base_subject": "Q3 plan — draft",
    "is_reply": false
  },
  "body": {
    "text": "Hi Bob,\n\nAttached is the Q3 draft. …",
    "html": null,
    "html_included": false,
    "html_available": true,
    "text_source": "text",
    "text_part_id": "1.1.1",
    "alternatives": [
      {
        "part_id": "1.1.1",
        "kind": "text",
        "text": null,
        "html": null,
        "same_as": "body.text",
        "decoded_size_bytes": 101,
        "decoded_sha256": "faccab8fa6…",
        "truncated": false
      },
      {
        "part_id": "1.1.2",
        "kind": "html",
        "text": null,
        "html": null,
        "same_as": null,
        "decoded_size_bytes": 158,
        "decoded_sha256": "aad511a4a5…",
        "truncated": false
      }
    ],
    "fragments": [
      {
        "id": "frag_67d37117136f2bca",
        "kind": "fresh",
        "quote_depth": 0,
        "part_id": "1.1.1",
        "byte_range": [0, 101],
        "sha256": "faccab8fa6…",
        "truncated": false
      }
    ],
    "truncation": {
      "max_body_bytes": 65536,
      "truncated": false,
      "omitted_fragment_ids": []
    }
  },
  "headers": [
    { "name": "From", "value": "Alice Example <alice@example.com>", "raw": " Alice Example <alice@example.com>\n" },
    { "name": "Subject", "value": "Q3 plan — draft", "raw": " =?utf-8?q?Q3_plan_=E2=80=94_draft?=\n" }
  ],
  "headers_omitted": 0,
  "parts": [
    {
      "part_id": "1.2",
      "kind": "attachment",
      "content_type": "application/pdf",
      "filename": "q3-plan.pdf",
      "safe_filename": "q3-plan.pdf",
      "disposition": "attachment",
      "content_id": null,
      "decoded_size_bytes": 150,
      "decoded_sha256": "a8d52b9e7c…",
      "extractable": true
    }
  ],
  "nested_messages": [],
  "diagnostics": []
}
```

## Reconstructing Threads

Email is usually read as a conversation, not isolated records. `email-cli thread`
reconstructs threads across the `.eml` files you supply — threading is always
explicit, never inferred from the filesystem:

```sh
email-cli thread inbox/*.eml | jq '.threads[].timeline'
```

Threading uses `Message-ID`, `In-Reply-To`, and `References`. It prefers
ID-based links, falls back from unresolved `In-Reply-To` to the last
`References` entry, and records missing parents or duplicate IDs as diagnostics.

Subject-only grouping is available, but intentionally opt-in:

```sh
email-cli thread inbox/*.eml --subject-fallback
```

Attached `message/rfc822` parts are extractable messages, but they are not
silently added to a thread. Thread membership only comes from files explicitly
supplied to the command.

## Attachments And Parts

Default JSON includes attachment metadata, not attachment contents:

```json
{
  "part_id": "1.2",
  "kind": "attachment",
  "content_type": "application/pdf",
  "filename": "invoice.pdf",
  "safe_filename": "invoice.pdf",
  "decoded_size_bytes": 12345,
  "decoded_sha256": "...",
  "extractable": true
}
```

Use `extract` to write decoded bytes:

```sh
email-cli extract message.eml --part 1.2 -o invoice.pdf
```

Part IDs are MIME-path IDs, not filenames or array indexes. This matters because
filenames can be absent, duplicated, malicious, or unsafe as filesystem paths.

## Prompt-Ready Text

Text output is designed for feeding models directly:

```sh
email-cli thread *.eml --format text --quotes drop
```

Quote handling:

- `--quotes keep`: preserve quoted content
- `--quotes collapse`: replace quoted runs with deterministic markers such as
  `[quoted content collapsed: 14 lines]`
- `--quotes drop`: omit quoted runs from text output

Quote detection is mechanical and covers the common plain-text patterns:
`>`-prefixed lines, `On … wrote:` attributions, `-----Original Message-----`
dividers, and Outlook-style top-post blocks (an underscore separator or a bare
`From:`/`Sent:`/`Subject:` header block, after which the rest of the body is
quoted).

JSON output always preserves fragment metadata, even when text rendering drops
or collapses quoted content.

## Batch-Friendly Behavior

Use `messages` for scripts and pipelines:

```sh
email-cli messages *.eml --format json
email-cli messages *.eml --format ndjson
```

Batch commands keep going after per-file read or parse failures and include
structured diagnostics in the output. If every supplied input fails,
`email-cli` exits nonzero.

## Diagnostics

Diagnostics carry stable codes so agents can branch on them without
string-matching prose:

| Code | Severity | Meaning |
| --- | --- | --- |
| `BODY_TRUNCATED` | info | Body text was cut at `--max-body-bytes`; see `body.truncation`. |
| `HTML_CONVERTED_TO_TEXT` | info | `body.text` was converted from an HTML body; pass `--html` for the raw HTML. |
| `TEXT_BODY_CONTAINS_HTML` | info | The text/plain body itself embeds raw HTML markup; consider the HTML alternative when `html_available` is true. |
| `PART_ENCODING_PROBLEM` | warning | The parser reported an encoding problem for a MIME part; see `location` for the part ID. |
| `MISSING_THREAD_PARENT` | warning | A referenced parent Message-ID was not among the supplied files. |
| `DUPLICATE_MESSAGE_ID` | warning | The same Message-ID occurs more than once across the supplied files. |
| `READ_FAILED` | error | An input file could not be read. |
| `PARSE_FAILED` | error | An input could not be parsed as an RFC 822 / RFC 5322 message. |

When the single-message command fails to read or parse its input under the
default JSON format, it prints a schema-versioned envelope containing one of
the error diagnostics above to stdout and exits nonzero — the same contract as
batch output, so agents never have to parse stderr.

## Exit Codes

- `0`: success, including batch runs where some inputs failed (check
  `diagnostics`)
- `1`: the single input could not be read or parsed, `extract` failed, or every
  batch input failed
- `2`: command-line usage error

## Schema Stability

`schema_version` is `MAJOR.MINOR`, currently `1.1`.

- Minor bumps are additive (new keys) or reduce default content in ways a flag
  can restore (for example `--headers all`). Existing keys keep their meaning.
- Major bumps may remove or rename keys or change their meaning.
- Consumers should ignore keys they do not recognize.

Every documented key is always present: repeated fields are empty arrays and
absent scalars are `null`, so `jq` paths never disappear between messages.

## Agent Skill

`skills/email-cli/` ships a skill that teaches coding agents when to reach for
`email-cli` instead of hand-rolling MIME parsing, plus jq patterns and safety
guidance for attachments. For Claude Code, copy or symlink it into
`~/.claude/skills/` (or a project's `.claude/skills/`); for OpenAI Codex, the
bundled `agents/openai.yaml` config points at the same skill.

## Design Principles

- Deterministic output: same input content and arguments produce the same data.
- Read-only operation: no mail is sent, modified, deleted, indexed, or fetched.
- Provenance first: output points back to source files, hashes, headers, body
  fragments, and MIME part IDs.
- Agent ergonomics: schema-versioned JSON, stable diagnostic codes, and
  predictable command shapes.
- Honest scope: email decoding and structure, not email understanding.

## Development

Run the test suite:

```sh
cargo test
```

Run lint checks:

```sh
cargo clippy --all-targets --all-features -- -D warnings
```

Format code:

```sh
cargo fmt
```
