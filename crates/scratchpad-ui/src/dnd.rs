use crate::panel::PanelController;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use scratchpad_core::*;
use std::{path::{Path, PathBuf}, rc::Rc};

pub type SubmitFn = Rc<dyn Fn(Object) -> bool>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropKind {
    UriList,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutboundFlavor {
    Native,
    Text,
}

pub fn install_inbound(
    root: &gtk::Overlay,
    panel: PanelController,
    submit: SubmitFn,
) {
    let formats = gtk::gdk::ContentFormats::new(&[
        "text/uri-list",
        "text/plain;charset=utf-8",
        "text/plain",
        "text/x-moz-url",
    ]);

    let target = gtk::DropTargetAsync::new(
        Some(formats),
        gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
    );

    target.connect_accept(|target, drop| {
        let Some(kind) = classify_drop(drop) else {
            return false;
        };

        let actions = match kind {
            DropKind::UriList => gtk::gdk::DragAction::COPY,
            DropKind::Text => {
                gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE
            }
        };
        target.set_actions(actions);

        if dnd_debug() {
            eprintln!(
                "[scratchpad-dnd] accept kind={kind:?} source_actions={:?} target_actions={:?} formats={}",
                drop.actions(),
                target.actions(),
                drop.formats().to_str()
            );
        }

        true
    });

    {
        let panel = panel.clone();
        let submit = submit.clone();

        target.connect_drop(move |_, drop, _, _| {
            let Some(kind) = classify_drop(drop) else {
                return false;
            };

            let final_action = choose_final_action(kind, drop.actions());
            if final_action.is_empty() {
                return false;
            }

            if dnd_debug() {
                eprintln!(
                    "[scratchpad-dnd] drop start kind={kind:?} negotiated={:?} final={:?}",
                    drop.actions(),
                    final_action
                );
            }

            panel.set_drag_active(true);

            let drop = drop.clone();
            let submit = submit.clone();
            let panel = panel.clone();

            glib::MainContext::default().spawn_local(async move {
                let result = read_foreign_drop(&drop, kind).await;
                let accepted = match result {
                    Ok((mime, bytes)) => {
                        if dnd_debug() {
                            eprintln!(
                                "[scratchpad-dnd] received mime={} bytes={}",
                                mime,
                                bytes.len()
                            );
                        }
                        payload_to_objects(&mime, &bytes)
                            .into_iter()
                            .fold(false, |any, object| submit(object) || any)
                    }
                    Err(err) => {
                        eprintln!("[scratchpad-dnd] read failed: {err:#}");
                        false
                    }
                };

                drop.finish(if accepted {
                    final_action
                } else {
                    gtk::gdk::DragAction::empty()
                });

                panel.set_drag_active(false);
            });

            true
        });
    }

    root.add_controller(target);
}

pub fn install_outbound<W: IsA<gtk::Widget>>(
    widget: &W,
    object: Object,
    panel: PanelController,
) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::COPY);

    source.connect_prepare(move |source, _, _| {
        // Pick one unambiguous representation at drag start. Normal drag uses
        // the object's native/file-like form so file managers work naturally.
        // Holding Ctrl *before starting the drag* forces semantic plain text,
        // which avoids editors interpreting URLs as files.
        let ctrl = source
            .current_event_state()
            .contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let flavor = choose_outbound_flavor(ctrl);
        let provider = drag_provider(&object, flavor);

        if dnd_debug() {
            match &provider {
                Some(provider) => eprintln!(
                    "[scratchpad-dnd] outbound prepare object={} ctrl={} flavor={flavor:?} formats={}",
                    object.id,
                    ctrl,
                    provider.formats().to_str()
                ),
                None => eprintln!(
                    "[scratchpad-dnd] outbound prepare object={} ctrl={} flavor={flavor:?} no-provider",
                    object.id,
                    ctrl
                ),
            }
        }

        provider
    });

    {
        let panel = panel.clone();
        source.connect_drag_begin(move |_, drag| {
            panel.set_drag_active(true);
            if dnd_debug() {
                eprintln!(
                    "[scratchpad-dnd] outbound drag-begin actions={:?}",
                    drag.actions()
                );
            }
        });
    }

    source.connect_drag_end(move |_, drag, delete_data| {
        if dnd_debug() {
            eprintln!(
                "[scratchpad-dnd] outbound drag-end selected={:?} delete_data={}",
                drag.selected_action(),
                delete_data
            );
        }
        panel.set_drag_active(false);
    });

    widget.add_controller(source);
}

fn classify_drop(drop: &gtk::gdk::Drop) -> Option<DropKind> {
    let formats = drop.formats();

    if formats.contain_mime_type("text/uri-list") {
        return Some(DropKind::UriList);
    }

    if formats.contain_mime_type("text/plain;charset=utf-8")
        || formats.contain_mime_type("text/plain")
        || formats.contain_mime_type("text/x-moz-url")
    {
        return Some(DropKind::Text);
    }

    None
}

fn choose_final_action(
    kind: DropKind,
    actions: gtk::gdk::DragAction,
) -> gtk::gdk::DragAction {
    if actions.contains(gtk::gdk::DragAction::COPY) {
        return gtk::gdk::DragAction::COPY;
    }

    if kind == DropKind::Text && actions.contains(gtk::gdk::DragAction::MOVE) {
        return gtk::gdk::DragAction::MOVE;
    }

    gtk::gdk::DragAction::empty()
}

async fn read_foreign_drop(
    drop: &gtk::gdk::Drop,
    kind: DropKind,
) -> anyhow::Result<(String, Vec<u8>)> {
    let requested: &[&str] = match kind {
        DropKind::UriList => &["text/uri-list"],
        DropKind::Text => &[
            "text/plain;charset=utf-8",
            "text/plain",
            "text/x-moz-url",
        ],
    };

    let (stream, mime) = drop
        .read_future(requested, glib::Priority::DEFAULT)
        .await?;

    let mut out = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await?;
        if chunk.is_empty() {
            break;
        }

        if out.len() + chunk.len() > max_drop_bytes() {
            anyhow::bail!("drop exceeded {} bytes", max_drop_bytes());
        }

        out.extend_from_slice(chunk.as_ref());
    }

    Ok((mime.to_string(), out))
}

fn payload_to_objects(mime: &str, bytes: &[u8]) -> Vec<Object> {
    if mime == "text/uri-list" {
        return uri_list_objects(bytes);
    }

    let mut text = String::from_utf8_lossy(bytes).into_owned();

    if mime == "text/x-moz-url" {
        if bytes.as_ref().windows(2).any(|window| window == [0, 0]) {
            let utf16 = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            text = String::from_utf16_lossy(&utf16);
        }
        text = text.lines().next().unwrap_or_default().to_string();
    }

    text_object(text).into_iter().collect()
}

fn uri_list_objects(bytes: &[u8]) -> Vec<Object> {
    let text = String::from_utf8_lossy(bytes);
    let mut objects = Vec::new();

    for line in text.lines() {
        let uri = line.trim();
        if uri.is_empty() || uri.starts_with('#') {
            continue;
        }

        let Some(parsed) = url::Url::parse(uri).ok() else {
            continue;
        };

        let object = if parsed.scheme() == "file" {
            parsed
                .to_file_path()
                .ok()
                .and_then(|path| object_from_path(&path))
        } else {
            object_from_uri(uri)
        };

        if let Some(object) = object {
            objects.push(object);
        }
    }

    objects
}

fn text_object(text: String) -> Option<Object> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    let source = SourceDescriptor::DragDrop {
        app_id: None,
        offered_mime_types: vec!["text/plain;charset=utf-8".into()],
    };

    if url::Url::parse(&text).is_ok() {
        Some(
            Object::new(ObjectKind::Url, Some(text.clone()), source)
                .with_representation(
                    "text/uri-list",
                    RepresentationRole::Primary,
                    StorageRef::Uri { uri: text.clone() },
                    Some(text.len() as u64),
                )
                .with_representation(
                    "text/plain;charset=utf-8",
                    RepresentationRole::Secondary,
                    StorageRef::InlineText { text: text.clone() },
                    Some(text.len() as u64),
                ),
        )
    } else {
        Some(
            Object::new(ObjectKind::Text, first_line(&text), source)
                .with_representation(
                    "text/plain;charset=utf-8",
                    RepresentationRole::Primary,
                    StorageRef::InlineText { text: text.clone() },
                    Some(text.len() as u64),
                ),
        )
    }
}

fn object_from_path(path: &Path) -> Option<Object> {
    let path = std::fs::canonicalize(path).ok()?;
    let metadata = std::fs::metadata(&path).ok()?;

    let kind = if metadata.is_dir() {
        ObjectKind::Directory
    } else {
        kind_for_path(&path)
    };

    let mime = if metadata.is_dir() {
        "inode/directory".to_string()
    } else {
        mime_for_path(&path)
    };

    let title = path.file_name().map(|s| s.to_string_lossy().into_owned());

    Some(
        Object::new(
            kind,
            title,
            SourceDescriptor::DragDrop {
                app_id: None,
                offered_mime_types: vec!["text/uri-list".into()],
            },
        )
        .with_representation(
            mime,
            RepresentationRole::Primary,
            StorageRef::ExternalPath {
                path: path.display().to_string(),
            },
            Some(metadata.len()),
        ),
    )
}

fn object_from_uri(uri: &str) -> Option<Object> {
    url::Url::parse(uri).ok()?;

    Some(
        Object::new(
            ObjectKind::Url,
            Some(uri.to_string()),
            SourceDescriptor::DragDrop {
                app_id: None,
                offered_mime_types: vec!["text/uri-list".into()],
            },
        )
        .with_representation(
            "text/uri-list",
            RepresentationRole::Primary,
            StorageRef::Uri {
                uri: uri.to_string(),
            },
            Some(uri.len() as u64),
        ),
    )
}

fn choose_outbound_flavor(ctrl: bool) -> OutboundFlavor {
    if ctrl {
        OutboundFlavor::Text
    } else {
        OutboundFlavor::Native
    }
}

fn drag_provider(
    object: &Object,
    flavor: OutboundFlavor,
) -> Option<gtk::gdk::ContentProvider> {
    match flavor {
        OutboundFlavor::Native => uri_provider(object)
            .or_else(|| semantic_text(object).map(text_provider)),
        OutboundFlavor::Text => semantic_text(object)
            .map(text_provider)
            .or_else(|| uri_provider(object)),
    }
}

fn text_provider(text: String) -> gtk::gdk::ContentProvider {
    let providers = [
        bytes_provider(
            "text/plain;charset=utf-8",
            text.as_bytes().to_vec(),
        ),
        bytes_provider(
            "text/plain",
            text.as_bytes().to_vec(),
        ),
    ];
    gtk::gdk::ContentProvider::new_union(&providers)
}

fn uri_provider(object: &Object) -> Option<gtk::gdk::ContentProvider> {
    let uri = outbound_uri(object).or_else(|| {
        full_text(object)
            .and_then(|text| materialize_text_export(object, &text))
    })?;

    Some(bytes_provider(
        "text/uri-list",
        format!("{uri}\r\n").into_bytes(),
    ))
}

fn semantic_text(object: &Object) -> Option<String> {
    full_text(object)
        .or_else(|| primary_uri(object))
        .or_else(|| external_path(object))
}

fn bytes_provider(mime: &str, data: Vec<u8>) -> gtk::gdk::ContentProvider {
    let bytes = glib::Bytes::from_owned(data);
    gtk::gdk::ContentProvider::for_bytes(mime, &bytes)
}

fn outbound_uri(object: &Object) -> Option<String> {
    if let Some(uri) = primary_uri(object) {
        return Some(uri);
    }

    external_path(object).and_then(|path| {
        url::Url::from_file_path(PathBuf::from(path))
            .ok()
            .map(|url| url.to_string())
    })
}

fn materialize_text_export(object: &Object, text: &str) -> Option<String> {
    let paths = Paths::discover();
    let export_dir = paths.exports();
    std::fs::create_dir_all(&export_dir).ok()?;

    let file = export_dir.join(format!("{}.txt", object.id));
    std::fs::write(&file, text).ok()?;

    url::Url::from_file_path(file)
        .ok()
        .map(|url| url.to_string())
}

fn full_text(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        if let StorageRef::InlineText { text } = &representation.storage {
            Some(text.clone())
        } else {
            None
        }
    })
}

fn primary_uri(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| match &representation.storage {
        StorageRef::Uri { uri } => Some(uri.clone()),
        _ => None,
    })
}

fn external_path(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| match &representation.storage {
        StorageRef::ExternalPath { path } => Some(path.clone()),
        _ => None,
    })
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .next()
        .map(|line| line.chars().take(80).collect())
}

fn kind_for_path(path: &Path) -> ObjectKind {
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

fn mime_for_path(path: &Path) -> String {
    match kind_for_path(path) {
        ObjectKind::Image => "image/*",
        ObjectKind::Video => "video/*",
        ObjectKind::Audio => "audio/*",
        ObjectKind::Pdf => "application/pdf",
        _ => "application/octet-stream",
    }
    .into()
}

fn max_drop_bytes() -> usize {
    std::env::var("SCRATCHPAD_MAX_DROP_BYTES")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(8 * 1024 * 1024)
}

fn dnd_debug() -> bool {
    std::env::var_os("SCRATCHPAD_DND_DEBUG").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_move_is_never_accepted() {
        assert!(choose_final_action(
            DropKind::UriList,
            gtk::gdk::DragAction::MOVE
        )
        .is_empty());
    }

    #[test]
    fn uri_prefers_copy() {
        assert_eq!(
            choose_final_action(
                DropKind::UriList,
                gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE
            ),
            gtk::gdk::DragAction::COPY
        );
    }

    #[test]
    fn text_can_accept_move_only_sources() {
        assert_eq!(
            choose_final_action(DropKind::Text, gtk::gdk::DragAction::MOVE),
            gtk::gdk::DragAction::MOVE
        );
    }

    #[test]
    fn outbound_modifier_selects_one_representation() {
        assert_eq!(
            choose_outbound_flavor(false),
            OutboundFlavor::Native
        );
        assert_eq!(
            choose_outbound_flavor(true),
            OutboundFlavor::Text
        );
    }
}
