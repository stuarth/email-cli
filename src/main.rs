use std::{io::Write, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use email_cli::{
    QuoteMode, RenderOptions, build_thread_envelope, extract_part_bytes, parse_message_bytes,
    read_file_or_stdin, read_required_file, render_message_text, render_thread_text,
};

#[derive(Debug, Parser)]
#[command(name = "email-cli")]
#[command(about = "Make .eml files legible to LLM agents and shell pipelines")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Message file to parse. Use '-' or omit the argument to read stdin.
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => run_single(cli),
        Some(Command::Thread {
            files,
            format,
            html,
            max_body_bytes,
            quotes,
            subject_fallback,
        }) => run_thread(
            files,
            format,
            html,
            max_body_bytes,
            quotes,
            subject_fallback,
        ),
        Some(Command::Messages {
            files,
            format,
            html,
            max_body_bytes,
            quotes,
        }) => run_messages(files, format, html, max_body_bytes, quotes),
        Some(Command::Extract { file, part, output }) => run_extract(file, &part, output),
    }
}

fn run_single(cli: Cli) -> Result<()> {
    let (path, raw) = read_file_or_stdin(cli.file.as_deref())?;
    let options = RenderOptions {
        include_html: cli.html,
        max_body_bytes: cli.max_body_bytes,
        quotes: cli.quotes.into(),
    };
    let message = parse_message_bytes(path, &raw, options)?;

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

fn run_thread(
    files: Vec<PathBuf>,
    format: SingleFormat,
    html: bool,
    max_body_bytes: usize,
    quotes: QuoteChoice,
    subject_fallback: bool,
) -> Result<()> {
    let options = RenderOptions {
        include_html: html,
        max_body_bytes,
        quotes: quotes.into(),
    };
    let mut messages = Vec::new();

    for file in files {
        let raw = read_required_file(&file)?;
        messages.push(parse_message_bytes(
            file.display().to_string(),
            &raw,
            options,
        )?);
    }

    let envelope = build_thread_envelope(messages, subject_fallback);
    match format {
        SingleFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        }
        SingleFormat::Text => {
            println!("{}", render_thread_text(&envelope, options.quotes));
        }
    }

    Ok(())
}

fn run_messages(
    files: Vec<PathBuf>,
    format: MessagesFormat,
    html: bool,
    max_body_bytes: usize,
    quotes: QuoteChoice,
) -> Result<()> {
    let options = RenderOptions {
        include_html: html,
        max_body_bytes,
        quotes: quotes.into(),
    };
    let mut messages = Vec::new();

    for file in files {
        let raw = read_required_file(&file)?;
        messages.push(parse_message_bytes(
            file.display().to_string(),
            &raw,
            options,
        )?);
    }

    match format {
        MessagesFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&messages)?);
        }
        MessagesFormat::Ndjson => {
            for message in messages {
                println!("{}", serde_json::to_string(&message)?);
            }
        }
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
