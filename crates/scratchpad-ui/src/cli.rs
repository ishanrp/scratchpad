use anyhow::{Context, Result};
use clap::Subcommand;
use scratchpad_core::*;
use std::{io::Read, path::PathBuf};
use uuid::Uuid;

#[derive(Subcommand, Debug)]
pub enum Command {
    Serve,
    Ping,
    List {
        #[arg(default_value_t = 100)]
        limit: usize,
    },
    Show {
        id: Uuid,
    },
    Remove {
        id: Uuid,
    },
    Pages,
    NewPage {
        name: String,
    },
    Page {
        id: Uuid,
    },
    AddText {
        #[arg(long)]
        stdin: bool,
        text: Vec<String>,
    },
    AddUrl {
        url: String,
    },
    AddPath {
        path: PathBuf,
    },
}

pub fn request(command: Command) -> Result<Request> {
    Ok(match command {
        Command::Serve => unreachable!("serve is handled by main"),
        Command::Ping => Request::Ping,
        Command::List { limit } => Request::ListObjects { limit },
        Command::Show { id } => Request::GetObject { object_id: id },
        Command::Remove { id } => Request::RemoveObject { object_id: id },
        Command::Pages => Request::ListPages,
        Command::NewPage { name } => Request::CreatePage { name },
        Command::Page { id } => Request::GetPage { page_id: id },
        Command::AddText { stdin, text } => {
            let value = if stdin {
                let mut value = String::new();
                std::io::stdin().read_to_string(&mut value)?;
                value
            } else {
                text.join(" ")
            };

            let object = Object::new(
                ObjectKind::Text,
                first_line(&value),
                SourceDescriptor::Cli,
            )
            .with_representation(
                "text/plain;charset=utf-8",
                RepresentationRole::Primary,
                StorageRef::InlineText {
                    text: value.clone(),
                },
                Some(value.len() as u64),
            );

            Request::AddObject {
                object,
                page_id: None,
                placement: None,
            }
        }
        Command::AddUrl { url } => {
            url::Url::parse(&url).context("invalid URL")?;
            let object = Object::new(
                ObjectKind::Url,
                Some(url.clone()),
                SourceDescriptor::Cli,
            )
            .with_representation(
                "text/uri-list",
                RepresentationRole::Primary,
                StorageRef::Uri { uri: url.clone() },
                Some(url.len() as u64),
            )
            .with_representation(
                "text/plain;charset=utf-8",
                RepresentationRole::Secondary,
                StorageRef::InlineText {
                    text: url.clone(),
                },
                Some(url.len() as u64),
            );

            Request::AddObject {
                object,
                page_id: None,
                placement: None,
            }
        }
        Command::AddPath { path } => {
            let path = std::fs::canonicalize(&path)
                .with_context(|| format!("cannot access {}", path.display()))?;
            let metadata = std::fs::metadata(&path)?;
            let kind = if metadata.is_dir() {
                ObjectKind::Directory
            } else {
                kind_for_path(&path)
            };
            let mime = if metadata.is_dir() {
                "inode/directory".into()
            } else {
                mime_for_path(&path)
            };

            let object = Object::new(
                kind,
                path.file_name()
                    .map(|x| x.to_string_lossy().into_owned()),
                SourceDescriptor::FileSystem {
                    path: path.display().to_string(),
                },
            )
            .with_representation(
                mime,
                RepresentationRole::Primary,
                StorageRef::ExternalPath {
                    path: path.display().to_string(),
                },
                Some(metadata.len()),
            );

            Request::AddObject {
                object,
                page_id: None,
                placement: None,
            }
        }
    })
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .next()
        .map(|line| line.chars().take(80).collect())
}

fn kind_for_path(path: &std::path::Path) -> ObjectKind {
    match path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "webp" | "gif" => ObjectKind::Image,
        "mp4" | "mkv" | "webm" | "mov" => ObjectKind::Video,
        "mp3" | "flac" | "wav" | "ogg" => ObjectKind::Audio,
        "pdf" => ObjectKind::Pdf,
        _ => ObjectKind::File,
    }
}

fn mime_for_path(path: &std::path::Path) -> String {
    match kind_for_path(path) {
        ObjectKind::Image => "image/*",
        ObjectKind::Video => "video/*",
        ObjectKind::Audio => "audio/*",
        ObjectKind::Pdf => "application/pdf",
        _ => "application/octet-stream",
    }
    .into()
}
