use crate::{Object, ObjectKind, Representation, StorageRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExternalAction { Copy }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalOffer { pub mime_type: String, pub source: OfferSource }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OfferSource { Representation { representation_id: uuid::Uuid }, PreMaterializeTextFile { representation_id: uuid::Uuid, suggested_name: String } }

pub struct ExternalDnDOfferResolver;
impl ExternalDnDOfferResolver {
    pub fn offers(object: &Object) -> Vec<ExternalOffer> {
        let mut out = vec![];
        for r in &object.representations {
            out.push(ExternalOffer { mime_type: r.mime_type.clone(), source: OfferSource::Representation { representation_id: r.id } });
            if r.mime_type.starts_with("text/plain") && matches!(r.storage, StorageRef::InlineText { .. }) {
                out.push(ExternalOffer { mime_type: "text/uri-list".into(), source: OfferSource::PreMaterializeTextFile { representation_id: r.id, suggested_name: "scratchpad.txt".into() } });
            }
        }
        out.sort_by(|a,b| a.mime_type.cmp(&b.mime_type));
        out.dedup_by(|a,b| a.mime_type == b.mime_type);
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InternalAction { CopyToDirectory, MoveToDirectory, OpenWithApp, InsertText, Transform { plugin_id: String, action_id: String } }

pub struct InternalTargetResolver;
impl InternalTargetResolver {
    pub fn builtin_actions(source: &Object, target: &Object) -> Vec<InternalAction> {
        match target.kind {
            ObjectKind::Directory if matches!(source.kind, ObjectKind::File | ObjectKind::Image | ObjectKind::Video | ObjectKind::Audio | ObjectKind::Pdf) => vec![InternalAction::CopyToDirectory, InternalAction::MoveToDirectory],
            ObjectKind::App => vec![InternalAction::OpenWithApp],
            ObjectKind::Tool => vec![],
            _ => vec![],
        }
    }

    pub fn preferred_text(rep: &[Representation]) -> Option<&Representation> {
        rep.iter().find(|r| r.mime_type == "text/plain;charset=utf-8").or_else(|| rep.iter().find(|r| r.mime_type == "text/plain"))
    }
}

#[cfg(test)] mod tests {
    use super::*; use crate::*;
    #[test] fn text_advertises_raw_and_uri() {
        let o=Object::new(ObjectKind::Text,None,SourceDescriptor::Cli).with_representation("text/plain",RepresentationRole::Primary,StorageRef::InlineText{text:"hello".into()},Some(5));
        let m=ExternalDnDOfferResolver::offers(&o).into_iter().map(|x|x.mime_type).collect::<Vec<_>>();
        assert!(m.contains(&"text/plain".into())); assert!(m.contains(&"text/uri-list".into()));
    }
    #[test] fn directory_target_has_copy_and_move() {
        let s=Object::new(ObjectKind::File,None,SourceDescriptor::Cli); let t=Object::new(ObjectKind::Directory,None,SourceDescriptor::Cli);
        assert_eq!(InternalTargetResolver::builtin_actions(&s,&t), vec![InternalAction::CopyToDirectory,InternalAction::MoveToDirectory]);
    }
}
