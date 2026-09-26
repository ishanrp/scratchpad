use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use scratchpad_core::*;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MIGRATION: &str = include_str!("../../../migrations/0001_init.sql");

pub struct Repository {
    conn: Connection,
}

impl Repository {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch(MIGRATION)?;

        let repo = Self { conn };
        repo.ensure_default_page()?;
        Ok(repo)
    }

    fn ensure_default_page(&self) -> Result<()> {
        let count: i64 =
            self.conn
                .query_row("SELECT count(*) FROM pages", [], |row| row.get(0))?;

        if count == 0 {
            self.create_page("Inbox")?;
        }

        Ok(())
    }

    pub fn create_page(&self, name: &str) -> Result<Page> {
        let name = normalized_page_name(name)?;
        let now = chrono::Utc::now();
        let id = Uuid::new_v4();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM pages",
            [],
            |row| row.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO pages(id,name,position,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                id.to_string(),
                name,
                position,
                now.to_rfc3339(),
                now.to_rfc3339()
            ],
        )?;

        Ok(Page {
            id,
            name,
            position,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn rename_page(&self, page_id: Uuid, name: &str) -> Result<()> {
        let name = normalized_page_name(name)?;
        let changed = self.conn.execute(
            "UPDATE pages SET name=?2, updated_at=?3 WHERE id=?1",
            params![
                page_id.to_string(),
                name,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;

        if changed == 0 {
            anyhow::bail!("page not found");
        }

        Ok(())
    }

    pub fn delete_page(&self, page_id: Uuid) -> Result<()> {
        let count: i64 =
            self.conn
                .query_row("SELECT count(*) FROM pages", [], |row| row.get(0))?;

        if count <= 1 {
            anyhow::bail!("Scratchpad must keep at least one tab");
        }

        let changed = self.conn.execute(
            "DELETE FROM pages WHERE id=?1",
            [page_id.to_string()],
        )?;

        if changed == 0 {
            anyhow::bail!("page not found");
        }

        Ok(())
    }

    pub fn restore_page(&self, snapshot: &PageSnapshot) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let page = &snapshot.page;

        tx.execute(
            "INSERT OR REPLACE INTO pages(id,name,position,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                page.id.to_string(),
                page.name,
                page.position,
                page.created_at.to_rfc3339(),
                chrono::Utc::now().to_rfc3339()
            ],
        )?;

        for (_, placement) in &snapshot.items {
            tx.execute(
                "INSERT OR REPLACE INTO page_items(
                    page_id,object_id,x,y,width,height,z_index,pinned
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    placement.page_id.to_string(),
                    placement.object_id.to_string(),
                    placement.x,
                    placement.y,
                    placement.width,
                    placement.height,
                    placement.z_index,
                    placement.pinned as i32,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn list_pages(&self) -> Result<Vec<Page>> {
        let mut statement = self.conn.prepare(
            "SELECT id,name,position,created_at,updated_at
             FROM pages
             ORDER BY position, created_at",
        )?;

        let pages = statement
            .query_map([], |row| {
                Ok(Page {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    name: row.get(1)?,
                    position: row.get(2)?,
                    created_at: row.get::<_, String>(3)?.parse().unwrap(),
                    updated_at: row.get::<_, String>(4)?.parse().unwrap(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(pages)
    }

    pub fn insert_object(
        &self,
        object: &Object,
        page_id: Option<Uuid>,
        placement: Option<&Placement>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO objects(
                id,kind,title,source_json,lifecycle,created_at,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                object.id.to_string(),
                serde_json::to_string(&object.kind)?,
                object.title,
                serde_json::to_string(&object.source)?,
                serde_json::to_string(&object.lifecycle)?,
                object.created_at.to_rfc3339(),
                object.updated_at.to_rfc3339()
            ],
        )?;

        for representation in &object.representations {
            tx.execute(
                "INSERT INTO representations(
                    id,object_id,mime_type,role,storage_kind,storage_json,size_bytes,created_at
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    representation.id.to_string(),
                    object.id.to_string(),
                    representation.mime_type,
                    serde_json::to_string(&representation.role)?,
                    storage_kind(&representation.storage),
                    serde_json::to_string(&representation.storage)?,
                    representation.size_bytes.map(|value| value as i64),
                    representation.created_at.to_rfc3339()
                ],
            )?;
        }

        let page_id = match page_id {
            Some(page_id) => page_id,
            None => {
                let id: String = tx.query_row(
                    "SELECT id FROM pages ORDER BY position, created_at LIMIT 1",
                    [],
                    |row| row.get(0),
                )?;
                Uuid::parse_str(&id)?
            }
        };

        let placement = placement.cloned().unwrap_or(Placement {
            page_id,
            object_id: object.id,
            x: 24.0,
            y: 24.0,
            width: 220.0,
            height: 150.0,
            z_index: 0,
            pinned: false,
        });

        tx.execute(
            "INSERT OR REPLACE INTO page_items(
                page_id,object_id,x,y,width,height,z_index,pinned
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                page_id.to_string(),
                object.id.to_string(),
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                placement.z_index,
                placement.pinned as i32
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn remove_placement(
        &self,
        page_id: Uuid,
        object_id: Uuid,
    ) -> Result<()> {
        let changed = self.conn.execute(
            "DELETE FROM page_items WHERE page_id=?1 AND object_id=?2",
            params![page_id.to_string(), object_id.to_string()],
        )?;

        if changed == 0 {
            anyhow::bail!("placement not found");
        }

        Ok(())
    }

    pub fn restore_placement(&self, placement: &Placement) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO page_items(
                page_id,object_id,x,y,width,height,z_index,pinned
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                placement.page_id.to_string(),
                placement.object_id.to_string(),
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                placement.z_index,
                placement.pinned as i32,
            ],
        )?;

        Ok(())
    }

    pub fn list_objects(&self, limit: usize) -> Result<Vec<Object>> {
        let mut statement = self
            .conn
            .prepare("SELECT id FROM objects ORDER BY created_at DESC LIMIT ?1")?;

        let ids = statement
            .query_map([limit as i64], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        ids.into_iter()
            .map(|id| {
                self.get_object(Uuid::parse_str(&id)?)?
                    .context("missing object")
            })
            .collect()
    }

    pub fn get_object(&self, id: Uuid) -> Result<Option<Object>> {
        let row = self
            .conn
            .query_row(
                "SELECT kind,title,source_json,lifecycle,created_at,updated_at
                 FROM objects WHERE id=?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;

        let Some((kind, title, source, lifecycle, created, updated)) = row else {
            return Ok(None);
        };

        let mut statement = self.conn.prepare(
            "SELECT id,mime_type,role,storage_json,size_bytes,created_at
             FROM representations
             WHERE object_id=?1
             ORDER BY created_at",
        )?;

        let rows = statement
            .query_map([id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let representations = rows
            .into_iter()
            .map(
                |(representation_id, mime, role, storage, size, created_at)|
                 -> Result<Representation> {
                    Ok(Representation {
                        id: Uuid::parse_str(&representation_id)?,
                        object_id: id,
                        mime_type: mime,
                        role: serde_json::from_str(&role)?,
                        storage: serde_json::from_str(&storage)?,
                        size_bytes: size.map(|value| value as u64),
                        created_at: created_at.parse()?,
                    })
                },
            )
            .collect::<Result<Vec<_>>>()?;

        Ok(Some(Object {
            id,
            kind: serde_json::from_str(&kind)?,
            title,
            source: serde_json::from_str(&source)?,
            lifecycle: serde_json::from_str(&lifecycle)?,
            created_at: created.parse()?,
            updated_at: updated.parse()?,
            representations,
        }))
    }

    pub fn page_snapshot(&self, page_id: Uuid) -> Result<PageSnapshot> {
        let page = self
            .list_pages()?
            .into_iter()
            .find(|page| page.id == page_id)
            .context("page not found")?;

        let mut statement = self.conn.prepare(
            "SELECT object_id,x,y,width,height,z_index,pinned
             FROM page_items
             WHERE page_id=?1
             ORDER BY z_index, rowid",
        )?;

        let rows = statement
            .query_map([page_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i32>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut items = Vec::new();

        for (object_id, x, y, width, height, z_index, pinned) in rows {
            let object_id = Uuid::parse_str(&object_id)?;
            if let Some(object) = self.get_object(object_id)? {
                items.push((
                    object,
                    Placement {
                        page_id,
                        object_id,
                        x,
                        y,
                        width,
                        height,
                        z_index,
                        pinned: pinned != 0,
                    },
                ));
            }
        }

        Ok(PageSnapshot { page, items })
    }

    pub fn remove_object(&self, id: Uuid) -> Result<()> {
        self.conn
            .execute("DELETE FROM objects WHERE id=?1", [id.to_string()])?;
        Ok(())
    }
}

fn normalized_page_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("tab name cannot be empty");
    }

    Ok(name.chars().take(64).collect())
}

fn storage_kind(storage: &StorageRef) -> &'static str {
    match storage {
        StorageRef::InlineText { .. } => "inline_text",
        StorageRef::ExternalPath { .. } => "external_path",
        StorageRef::ManagedBlob { .. } => "managed_blob",
        StorageRef::Uri { .. } => "uri",
        StorageRef::DesktopApp { .. } => "desktop_app",
        StorageRef::Virtual { .. } => "virtual",
    }
}

pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn put_bytes(&self, data: &[u8]) -> Result<String> {
        let digest = blake3::hash(data).to_hex().to_string();
        let path = self.root.join(&digest[..2]).join(&digest[2..]);

        if !path.exists() {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut file = fs::File::create(&path)?;
            file.write_all(data)?;
        }

        Ok(digest)
    }

    pub fn path_for(&self, digest: &str) -> PathBuf {
        self.root.join(&digest[..2]).join(&digest[2..])
    }
}
