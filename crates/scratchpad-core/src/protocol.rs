use crate::{Object, Page, PageSnapshot, Placement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub id: Uuid,
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(request: Request) -> Self {
        Self {
            id: Uuid::new_v4(),
            request,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub id: Uuid,
    pub response: Response,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Ping,

    ListPages,
    CreatePage { name: String },
    RenamePage { page_id: Uuid, name: String },
    DeletePage { page_id: Uuid },
    RestorePage { snapshot: PageSnapshot },
    GetPage { page_id: Uuid },

    AddObject {
        object: Object,
        page_id: Option<Uuid>,
        placement: Option<Placement>,
    },
    RemovePlacement {
        page_id: Uuid,
        object_id: Uuid,
    },
    RestorePlacement {
        placement: Placement,
    },

    ListObjects { limit: usize },
    GetObject { object_id: Uuid },
    RemoveObject { object_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Pages(Vec<Page>),
    Page(PageSnapshot),
    Objects(Vec<Object>),
    Object(Object),
    Created { id: Uuid },
    Updated,
    Removed,
    Error { message: String },
}
