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
  [--quotes keep|collapse|drop]

email-cli thread FILE... \
  [--format json|text] \
  [--html] \
  [--max-body-bytes N] \
  [--quotes keep|collapse|drop] \
  [--subject-fallback]

email-cli messages FILE... \
  [--format json|ndjson] \
  [--html] \
  [--max-body-bytes N] \
  [--quotes keep|collapse|drop]

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

Message IDs are normalized without surrounding angle brackets. Dates are
normalized to UTC RFC3339 in `message.date`, while the original Date header is
kept in `message.date_original`.

## Threads By Default, When You Ask For Conversations

Email is usually read as a conversation, not isolated records. `email-cli thread`
reconstructs threads across the `.eml` files you supply:

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
- `--quotes collapse`: replace quoted runs with deterministic markers
- `--quotes drop`: omit quoted runs from text output

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
