use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind { Text, Url, Image, Video, Audio, Pdf, File, Directory, App, Tool, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle { Available, Changed, Missing, PermissionDenied, Offline, MovedKnown }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
pub enum SourceDescriptor {
    DragDrop { app_id: Option<String>, offered_mime_types: Vec<String> },
    Clipboard { mime_types: Vec<String> },
    Cli,
    FileSystem { path: String },
    Generated { producer: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum RepresentationRole { Primary, Secondary, Preview, Export }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
pub enum StorageRef {
    InlineText { text: String },
    ExternalPath { path: String },
    ManagedBlob { digest: String },
    Uri { uri: String },
    DesktopApp { desktop_id: String },
    Virtual { generator: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Representation {
    pub id: Uuid,
    pub object_id: Uuid,
    pub mime_type: String,
    pub role: RepresentationRole,
    pub storage: StorageRef,
    pub size_bytes: Option<u64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub id: Uuid,
    pub kind: ObjectKind,
    pub title: Option<String>,
    pub source: SourceDescriptor,
    pub lifecycle: Lifecycle,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub representations: Vec<Representation>,
}

impl Object {
    pub fn new(kind: ObjectKind, title: Option<String>, source: SourceDescriptor) -> Self {
        let now = Utc::now();
        Self { id: Uuid::new_v4(), kind, title, source, lifecycle: Lifecycle::Available, created_at: now, updated_at: now, representations: vec![] }
    }

    pub fn with_representation(mut self, mime_type: impl Into<String>, role: RepresentationRole, storage: StorageRef, size_bytes: Option<u64>) -> Self {
        self.representations.push(Representation { id: Uuid::new_v4(), object_id: self.id, mime_type: mime_type.into(), role, storage, size_bytes, created_at: Utc::now() });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page { pub id: Uuid, pub name: String, pub position: i64, pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement { pub page_id: Uuid, pub object_id: Uuid, pub x: f64, pub y: f64, pub width: f64, pub height: f64, pub z_index: i64, pub pinned: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSnapshot { pub page: Page, pub items: Vec<(Object, Placement)> }
