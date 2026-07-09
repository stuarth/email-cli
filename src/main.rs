use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
};

use anyhow::{Result, anyhow};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use email_cli::{
    DiagnosticDto, HeaderScope, MessageDto, QuoteMode, RenderOptions, SCHEMA_VERSION,
    build_messages_envelope, build_thread_envelope, extract_part_bytes, parse_message_bytes,
    read_file_or_stdin, read_required_file, render_message_text, render_thread_text,
};

#[derive(Debug, Parser)]
#[command(name = "email-cli")]
#[command(about = "Make .eml files legible to LLM agents and shell pipelines")]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Message file to parse. Use '-' for stdin, or omit when stdin is piped.
    file: Option<String>,

    /// Output format for a single message.
    #[arg(long, value_enum, default_value_t = SingleFormat::Json)]
    format: SingleFormat,

    /// Include raw HTML body content in JSON output.
    #[arg(long)]
    html: bool,

    /// Maximum decoded body bytes to include.
    #[arg(long, default_value_t = email_cli::DEFAULT_MAX_BODY_BYTES)]
    max_body_bytes: usize,

    /// Control quoted-reply rendering in text output.
    #[arg(long, value_enum, default_value_t = QuoteChoice::Keep)]
    quotes: QuoteChoice,

    /// Which headers to include in JSON output.
    #[arg(long, value_enum, default_value_t = HeaderChoice::Standard)]
    headers: HeaderChoice,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Reconstruct conversations from explicitly supplied message files.
    Thread {
        #[arg(required = true)]
        files: Vec<PathBuf>,

        #[arg(long, value_enum, default_value_t = SingleFormat::Json)]
        format: SingleFormat,

        #[arg(long)]
        html: bool,

        #[arg(long, default_value_t = email_cli::DEFAULT_MAX_BODY_BYTES)]
        max_body_bytes: usize,

        #[arg(long, value_enum, default_value_t = QuoteChoice::Keep)]
        quotes: QuoteChoice,

        #[arg(long, value_enum, default_value_t = HeaderChoice::Standard)]
        headers: HeaderChoice,

        #[arg(long)]
        subject_fallback: bool,
    },

    /// Emit flat message records for scripts and pipelines.
    Messages {
        #[arg(required = true)]
        files: Vec<PathBuf>,

        #[arg(long, value_enum, default_value_t = MessagesFormat::Json)]
        format: MessagesFormat,

        #[arg(long)]
        html: bool,

        #[arg(long, default_value_t = email_cli::DEFAULT_MAX_BODY_BYTES)]
        max_body_bytes: usize,

        #[arg(long, value_enum, default_value_t = QuoteChoice::Keep)]
        quotes: QuoteChoice,

        #[arg(long, value_enum, default_value_t = HeaderChoice::Standard)]
        headers: HeaderChoice,
    },

    /// Write a decoded MIME part to a file or stdout.
    Extract {
        file: PathBuf,

        #[arg(long)]
        part: String,

        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SingleFormat {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MessagesFormat {
    Json,
    Ndjson,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum QuoteChoice {
    Keep,
    Collapse,
    Drop,
}

impl From<QuoteChoice> for QuoteMode {
    fn from(value: QuoteChoice) -> Self {
        match value {
            QuoteChoice::Keep => QuoteMode::Keep,
            QuoteChoice::Collapse => QuoteMode::Collapse,
            QuoteChoice::Drop => QuoteMode::Drop,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HeaderChoice {
    Standard,
    All,
}

impl From<HeaderChoice> for HeaderScope {
    fn from(value: HeaderChoice) -> Self {
        match value {
            HeaderChoice::Standard => HeaderScope::Standard,
            HeaderChoice::All => HeaderScope::All,
        }
    }
}

fn main() {
    if let Err(err) = run() {
        // Print the context chain only. anyhow's Debug formatting appends a
        // stack backtrace whenever RUST_BACKTRACE is set, which buries the
        // message for both humans and agents.
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if should_print_help_for_no_args(&cli, std::io::stdin().is_terminal()) {
        let mut command = Cli::command();
        command.print_help()?;
        println!();
        return Ok(());
    }

    match cli.command {
        None => run_single(cli),
        Some(Command::Thread {
            files,
            format,
            html,
            max_body_bytes,
            quotes,
            headers,
            subject_fallback,
        }) => run_thread(
            files,
            format,
            html,
            max_body_bytes,
            quotes,
            headers,
            subject_fallback,
        ),
        Some(Command::Messages {
            files,
            format,
            html,
            max_body_bytes,
            quotes,
            headers,
        }) => run_messages(files, format, html, max_body_bytes, quotes, headers),
        Some(Command::Extract { file, part, output }) => run_extract(file, &part, output),
    }
}

fn run_single(cli: Cli) -> Result<()> {
    let emit_json_errors = matches!(cli.format, SingleFormat::Json);
    let (path, raw) = match read_file_or_stdin(cli.file.as_deref()) {
        Ok(input) => input,
        Err(err) => {
            let location = cli.file.as_deref().unwrap_or("-");
            return fail_single(emit_json_errors, "READ_FAILED", location, err);
        }
    };
    let options = RenderOptions {
        include_html: cli.html,
        max_body_bytes: cli.max_body_bytes,
        quotes: cli.quotes.into(),
        headers: cli.headers.into(),
    };
    let message = match parse_message_bytes(path.clone(), &raw, options) {
        Ok(message) => message,
        Err(err) => return fail_single(emit_json_errors, "PARSE_FAILED", &path, err),
    };

    match cli.format {
        SingleFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&message)?);
        }
        SingleFormat::Text => {
            println!("{}", render_message_text(&message, options.quotes));
        }
    }

    Ok(())
}

/// Single-message failures still exit nonzero, but JSON consumers get the
/// same schema-versioned diagnostics envelope batch commands emit instead of
/// having to parse stderr prose.
fn fail_single(emit_json: bool, code: &str, location: &str, err: anyhow::Error) -> Result<()> {
    if emit_json {
        let envelope = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "diagnostics": [DiagnosticDto::error(
                code,
                format!("{err:#}"),
                None,
                Some(location),
            )],
        });
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    }
    Err(err)
}

fn run_thread(
    files: Vec<PathBuf>,
    format: SingleFormat,
    html: bool,
    max_body_bytes: usize,
    quotes: QuoteChoice,
    headers: HeaderChoice,
    subject_fallback: bool,
) -> Result<()> {
    let options = RenderOptions {
        include_html: html,
        max_body_bytes,
        quotes: quotes.into(),
        headers: headers.into(),
    };
    let (messages, diagnostics) = parse_files_lossy(files, options);

    let mut envelope = build_thread_envelope(messages, subject_fallback);
    envelope.diagnostics.extend(diagnostics);
    match format {
        SingleFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        }
        SingleFormat::Text => {
            println!("{}", render_thread_text(&envelope, options.quotes));
        }
    }

    if envelope.threads.is_empty() && !envelope.diagnostics.is_empty() {
        return Err(anyhow!("all input files failed"));
    }

    Ok(())
}

fn run_messages(
    files: Vec<PathBuf>,
    format: MessagesFormat,
    html: bool,
    max_body_bytes: usize,
    quotes: QuoteChoice,
    headers: HeaderChoice,
) -> Result<()> {
    let options = RenderOptions {
        include_html: html,
        max_body_bytes,
        quotes: quotes.into(),
        headers: headers.into(),
    };
    let (messages, diagnostics) = parse_files_lossy(files, options);
    let envelope = build_messages_envelope(messages, diagnostics);
    let all_inputs_failed = envelope.messages.is_empty() && !envelope.diagnostics.is_empty();

    match format {
        MessagesFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        }
        MessagesFormat::Ndjson => {
            for message in envelope.messages {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "schema_version": SCHEMA_VERSION,
                        "record_type": "message",
                        "message": message,
                    }))?
                );
            }
            for diagnostic in envelope.diagnostics {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "schema_version": SCHEMA_VERSION,
                        "record_type": "diagnostic",
                        "diagnostics": [diagnostic],
                    }))?
                );
            }
        }
    }

    if all_inputs_failed {
        return Err(anyhow!("all input files failed"));
    }

    Ok(())
}

fn run_extract(file: PathBuf, part: &str, output: Option<PathBuf>) -> Result<()> {
    let raw = read_required_file(&file)?;
    let bytes = extract_part_bytes(&raw, part)?;
    match output {
        Some(output) => {
            std::fs::write(&output, bytes)?;
        }
        None => {
            std::io::stdout().write_all(&bytes)?;
        }
    }
    Ok(())
}

fn parse_files_lossy(
    files: Vec<PathBuf>,
    options: RenderOptions,
) -> (Vec<MessageDto>, Vec<DiagnosticDto>) {
    let mut messages = Vec::new();
    let mut diagnostics = Vec::new();

    for file in files {
        let location = file.display().to_string();
        match read_required_file(&file) {
            Ok(raw) => match parse_message_bytes(location.clone(), &raw, options) {
                Ok(message) => messages.push(message),
                Err(err) => diagnostics.push(DiagnosticDto::error(
                    "PARSE_FAILED",
                    format!("Failed to parse {location}: {err:#}"),
                    None,
                    Some(&location),
                )),
            },
            Err(err) => diagnostics.push(DiagnosticDto::error(
                "READ_FAILED",
                format!("Failed to read {location}: {err:#}"),
                None,
                Some(&location),
            )),
        }
    }

    diagnostics.sort_by(|a, b| a.location.cmp(&b.location).then(a.code.cmp(&b.code)));
    (messages, diagnostics)
}

fn should_print_help_for_no_args(cli: &Cli, stdin_is_terminal: bool) -> bool {
    cli.command.is_none() && cli.file.is_none() && stdin_is_terminal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_prints_help_only_for_interactive_stdin() {
        let cli = Cli::parse_from(["email-cli"]);

        assert!(should_print_help_for_no_args(&cli, true));
        assert!(!should_print_help_for_no_args(&cli, false));
    }

    #[test]
    fn explicit_stdin_arg_does_not_print_help() {
        let cli = Cli::parse_from(["email-cli", "-"]);

        assert!(!should_print_help_for_no_args(&cli, true));
    }
}
