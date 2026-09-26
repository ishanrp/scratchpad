use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use scratchpad_core::*;
use std::{fs, io::Write, path::{Path,PathBuf}};
use uuid::Uuid;

const MIGRATION: &str = include_str!("../../../migrations/0001_init.sql");

pub struct Repository { conn: Connection }
impl Repository {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(p)=path.parent(){ fs::create_dir_all(p)?; }
        let conn=Connection::open(path)?; conn.execute_batch(MIGRATION)?;
        let repo=Self{conn}; repo.ensure_default_page()?; Ok(repo)
    }
    fn ensure_default_page(&self)->Result<()> { let n:i64=self.conn.query_row("SELECT count(*) FROM pages",[],|r|r.get(0))?; if n==0 { self.create_page("Inbox")?; } Ok(()) }
    pub fn create_page(&self,name:&str)->Result<Page>{
        let now=chrono::Utc::now(); let id=Uuid::new_v4(); let pos:i64=self.conn.query_row("SELECT COALESCE(MAX(position),-1)+1 FROM pages",[],|r|r.get(0))?;
        self.conn.execute("INSERT INTO pages(id,name,position,created_at,updated_at) VALUES(?1,?2,?3,?4,?5)",params![id.to_string(),name,pos,now.to_rfc3339(),now.to_rfc3339()])?;
        Ok(Page{id,name:name.into(),position:pos,created_at:now,updated_at:now})
    }
    pub fn list_pages(&self)->Result<Vec<Page>> { let mut st=self.conn.prepare("SELECT id,name,position,created_at,updated_at FROM pages ORDER BY position")?; let xs=st.query_map([],|r|Ok(Page{id:Uuid::parse_str(&r.get::<_,String>(0)?).unwrap(),name:r.get(1)?,position:r.get(2)?,created_at:r.get::<_,String>(3)?.parse().unwrap(),updated_at:r.get::<_,String>(4)?.parse().unwrap()}))?.collect::<rusqlite::Result<Vec<_>>>()?; Ok(xs) }
    pub fn insert_object(&self,o:&Object,page_id:Option<Uuid>,placement:Option<&Placement>)->Result<()> {
        let tx=self.conn.unchecked_transaction()?;
        tx.execute("INSERT INTO objects(id,kind,title,source_json,lifecycle,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![o.id.to_string(),serde_json::to_string(&o.kind)?,o.title,serde_json::to_string(&o.source)?,serde_json::to_string(&o.lifecycle)?,o.created_at.to_rfc3339(),o.updated_at.to_rfc3339()])?;
        for r in &o.representations { tx.execute("INSERT INTO representations(id,object_id,mime_type,role,storage_kind,storage_json,size_bytes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![r.id.to_string(),o.id.to_string(),r.mime_type,serde_json::to_string(&r.role)?,storage_kind(&r.storage),serde_json::to_string(&r.storage)?,r.size_bytes.map(|v|v as i64),r.created_at.to_rfc3339()])?; }
        let pid=match page_id {Some(x)=>x,None=>{let s:String=tx.query_row("SELECT id FROM pages ORDER BY position LIMIT 1",[],|r|r.get(0))?;Uuid::parse_str(&s)?}};
        let p=placement.cloned().unwrap_or(Placement{page_id:pid,object_id:o.id,x:24.0,y:24.0,width:220.0,height:150.0,z_index:0,pinned:false});
        tx.execute("INSERT OR REPLACE INTO page_items(page_id,object_id,x,y,width,height,z_index,pinned) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![pid.to_string(),o.id.to_string(),p.x,p.y,p.width,p.height,p.z_index,p.pinned as i32])?; tx.commit()?; Ok(())
    }
    pub fn list_objects(&self,limit:usize)->Result<Vec<Object>> { let mut st=self.conn.prepare("SELECT id FROM objects ORDER BY created_at DESC LIMIT ?1")?; let ids=st.query_map([limit as i64],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?; ids.into_iter().map(|s|self.get_object(Uuid::parse_str(&s).unwrap())?.context("missing object")).collect() }
    pub fn get_object(&self,id:Uuid)->Result<Option<Object>> {
        let row=self.conn.query_row("SELECT kind,title,source_json,lifecycle,created_at,updated_at FROM objects WHERE id=?1",[id.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?))).optional()?;
        let Some((kind,title,source,lifecycle,created,updated))=row else{return Ok(None)};
         let mut st=self.conn.prepare("SELECT id,mime_type,role,storage_json,size_bytes,created_at FROM representations WHERE object_id=?1 ORDER BY created_at")?;
        let rs=st.query_map([id.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,String>(5)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let representations=rs.into_iter().map(|(rid,mime,role,storage,size,at)|->Result<Representation>{Ok(Representation{id:Uuid::parse_str(&rid)?,object_id:id,mime_type:mime,role:serde_json::from_str(&role)?,storage:serde_json::from_str(&storage)?,size_bytes:size.map(|x|x as u64),created_at:at.parse()?})}).collect::<Result<Vec<_>>>()?;
        Ok(Some(Object{id,kind:serde_json::from_str(&kind)?,title,source:serde_json::from_str(&source)?,lifecycle:serde_json::from_str(&lifecycle)?,created_at:created.parse()?,updated_at:updated.parse()?,representations}))
    }
    pub fn page_snapshot(&self,page_id:Uuid)->Result<PageSnapshot>{ let page=self.list_pages()?.into_iter().find(|p|p.id==page_id).context("page not found")?; let mut st=self.conn.prepare("SELECT object_id,x,y,width,height,z_index,pinned FROM page_items WHERE page_id=?1 ORDER BY z_index")?; let rows=st.query_map([page_id.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,f64>(1)?,r.get::<_,f64>(2)?,r.get::<_,f64>(3)?,r.get::<_,f64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i32>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?; let mut items=vec![]; for (oid,x,y,w,h,z,p) in rows { let id=Uuid::parse_str(&oid)?; if let Some(o)=self.get_object(id)? {items.push((o,Placement{page_id,object_id:id,x,y,width:w,height:h,z_index:z,pinned:p!=0}));}} Ok(PageSnapshot{page,items}) }
    pub fn remove_object(&self,id:Uuid)->Result<()> { self.conn.execute("DELETE FROM objects WHERE id=?1",[id.to_string()])?; Ok(()) }
}
fn storage_kind(s:&StorageRef)->&'static str {match s{StorageRef::InlineText{..}=>"inline_text",StorageRef::ExternalPath{..}=>"external_path",StorageRef::ManagedBlob{..}=>"managed_blob",StorageRef::Uri{..}=>"uri",StorageRef::DesktopApp{..}=>"desktop_app",StorageRef::Virtual{..}=>"virtual"}}

pub struct BlobStore{root:PathBuf}
impl BlobStore{pub fn new(root:PathBuf)->Result<Self>{fs::create_dir_all(&root)?;Ok(Self{root})} pub fn put_bytes(&self,data:&[u8])->Result<String>{let d=blake3::hash(data).to_hex().to_string();let p=self.root.join(&d[..2]).join(&d[2..]);if !p.exists(){fs::create_dir_all(p.parent().unwrap())?;let mut f=fs::File::create(&p)?;f.write_all(data)?;}Ok(d)} pub fn path_for(&self,d:&str)->PathBuf{self.root.join(&d[..2]).join(&d[2..])}}
