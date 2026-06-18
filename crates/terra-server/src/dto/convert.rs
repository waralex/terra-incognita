//! Conversions between DTOs and terra-core domain types.

use terra_core::command::executor::checkout::CheckoutOutput;
use terra_core::command::input::checkout::CheckoutInput;
use terra_core::command::input::entity_history::EntityHistoryQuery;
use terra_core::command::input::grep_entities::{GrepEntitiesQuery, GrepScope};
use terra_core::command::input::transaction::{DeleteItem, TouchItem, TransactionInput};
use terra_core::domain::branch::Branch;
use terra_core::domain::entity::{Entity, PropertyValue, SimilarEntity};
use terra_core::domain::entity_history::EntityHistoryEntry;
use terra_core::domain::managed::Managed;
use terra_core::domain::transaction::{Transaction, TransactionDetail};
use terra_core::domain::tx_meta::TxMeta;
use terra_core::io::slug::Slug;

use crate::dto::request::{
    CheckoutReq, EntityHistoryReq, EntityReq, GrepEntitiesReq, ManagedReq, TransactionReq,
};
use crate::dto::response::{
    BranchRes, CheckoutRes, DeletedEntityRes, EntityHistoryEntryRes, EntityRes, ManagedRes,
    PropertyValueRes, SimilarEntityRes, TouchedEntityRes, TransactionDetailRes, TransactionRes,
    TxMetaRes,
};

// --- Request → Domain ---

fn parse_slug(s: &str) -> Result<Slug, String> {
    s.parse::<Slug>().map_err(|e| e.to_string())
}

pub fn transaction_req_to_input(req: TransactionReq) -> Result<TransactionInput, String> {
    let mut input = TransactionInput::new(req.meta);
    for e in req.create {
        input = input.create_entity(entity_req_to_domain(e)?);
    }
    for e in req.update {
        input = input.update_entity(entity_req_to_domain(e)?);
    }
    for m in req.create_managed {
        input = input.create_managed(managed_req_to_domain(m)?);
    }
    for m in req.update_managed {
        input = input.update_managed(managed_req_to_domain(m)?);
    }
    for d in req.delete {
        input = input.delete_entity(DeleteItem::new(parse_slug(&d.entity)?, d.reasoning));
    }
    for t in req.touch {
        input = input.touch(TouchItem::new(parse_slug(&t.entity)?, t.reasoning));
    }
    Ok(input)
}

pub fn checkout_req_to_input(req: CheckoutReq) -> Result<CheckoutInput, String> {
    let slug = parse_slug(&req.slug)?;
    let transaction = transaction_req_to_input(req.transaction)?;
    Ok(CheckoutInput::new(
        slug,
        req.meta,
        req.created_from_tx,
        transaction,
    ))
}

fn entity_req_to_domain(req: EntityReq) -> Result<Entity, String> {
    let slug = parse_slug(&req.slug)?;
    let props = req
        .properties
        .into_iter()
        .map(|p| {
            Ok(PropertyValue {
                property: parse_slug(&p.property)?,
                value: p.value,
                context: (),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Entity::new(slug, req.description, props, req.meta).with_status(req.status))
}

fn managed_req_to_domain(req: ManagedReq) -> Result<Managed, String> {
    let type_name = parse_slug(&req.type_name)?;
    let slug = parse_slug(&req.slug)?;
    Ok(Managed::new(type_name, slug, req.state, req.fields))
}

// --- Domain → Response ---

fn tx_meta_to_res(meta: TxMeta) -> TxMetaRes {
    TxMetaRes {
        tx_id: meta.tx_id,
        branch: meta.branch.to_string(),
        reasoning: meta.reasoning,
        time: meta.time,
        status: meta.status,
    }
}

pub fn transaction_to_res(tx: Transaction<TxMeta>) -> TransactionRes {
    TransactionRes {
        meta: tx.meta,
        context: tx_meta_to_res(tx.context),
    }
}

pub fn entity_to_res(e: Entity<TxMeta>) -> EntityRes {
    let properties = e
        .properties
        .into_iter()
        .map(|p| PropertyValueRes {
            property: p.property.to_string(),
            value: p.value,
            context: tx_meta_to_res(p.context),
        })
        .collect();
    EntityRes {
        slug: e.slug.to_string(),
        description: e.description,
        properties,
        meta: e.meta,
        context: tx_meta_to_res(e.context),
    }
}

pub fn branch_to_res(b: Branch<TxMeta>) -> BranchRes {
    BranchRes {
        slug: b.slug.to_string(),
        parent: b.parent.to_string(),
        meta: b.meta,
        context: tx_meta_to_res(b.context),
    }
}

pub fn managed_to_res(m: Managed<TxMeta>) -> ManagedRes {
    ManagedRes {
        type_name: m.type_name.to_string(),
        slug: m.slug.to_string(),
        state: m.state,
        fields: m.fields,
        context: tx_meta_to_res(m.context),
    }
}

pub fn transaction_detail_to_res(detail: TransactionDetail) -> TransactionDetailRes {
    TransactionDetailRes {
        meta: detail.meta,
        branch: detail.branch.to_string(),
        context: tx_meta_to_res(detail.context),
        created: detail.created.into_iter().map(entity_to_res).collect(),
        updated: detail.updated.into_iter().map(entity_to_res).collect(),
        deleted: detail
            .deleted
            .into_iter()
            .map(|d| DeletedEntityRes {
                slug: d.slug.to_string(),
                meta: d.meta,
                reasoning: d.reasoning,
                context: tx_meta_to_res(d.context),
            })
            .collect(),
        touched: detail
            .touched
            .into_iter()
            .map(|t| TouchedEntityRes {
                slug: t.slug.to_string(),
                reasoning: t.reasoning,
            })
            .collect(),
        created_managed: detail
            .created_managed
            .into_iter()
            .map(managed_to_res)
            .collect(),
        updated_managed: detail
            .updated_managed
            .into_iter()
            .map(managed_to_res)
            .collect(),
    }
}

pub fn checkout_to_res(out: CheckoutOutput) -> CheckoutRes {
    CheckoutRes {
        branch: out.branch.to_string(),
        created_from_tx: out.created_from_tx,
        transaction: transaction_to_res(out.transaction),
    }
}

pub fn similar_to_res(items: Vec<SimilarEntity<TxMeta>>) -> Vec<SimilarEntityRes> {
    items
        .into_iter()
        .map(|s| SimilarEntityRes {
            entity: entity_to_res(s.entity),
            similarity: s.similarity,
            matched_query: s.matched_query,
        })
        .collect()
}

pub fn entity_history_req_to_query(req: EntityHistoryReq) -> Result<EntityHistoryQuery, String> {
    let entity = parse_slug(&req.entity)?;
    let mut query = EntityHistoryQuery::new(entity, req.limit);
    if let Some(prop) = req.property {
        query = query.with_property(parse_slug(&prop)?);
    }
    if let Some(at_tx) = req.at_tx {
        query = query.with_at_tx(at_tx);
    }
    query.tx_id_from = req.tx_id_from;
    query.tx_id_to = req.tx_id_to;
    Ok(query)
}

pub fn grep_entities_req_to_query(req: GrepEntitiesReq) -> Result<GrepEntitiesQuery, String> {
    let scope = match req.scope {
        Some(fields) if !fields.is_empty() => parse_grep_scope(&fields)?,
        _ => GrepScope::default(),
    };
    let mut query = GrepEntitiesQuery::new(req.pattern, req.limit)
        .scope(scope)
        .include_properties(req.properties);
    if let Some(at_tx) = req.at_tx {
        query = query.at_tx(at_tx);
    }
    Ok(query)
}

fn parse_grep_scope(fields: &[String]) -> Result<GrepScope, String> {
    let mut scope = GrepScope {
        slug: false,
        property: false,
        value: false,
        reasoning: false,
    };
    for field in fields {
        match field.as_str() {
            "slug" => scope.slug = true,
            "property" => scope.property = true,
            "value" => scope.value = true,
            "reasoning" => scope.reasoning = true,
            other => {
                return Err(format!(
                    "unknown grep field \"{other}\": expected slug, property, value, or reasoning"
                ))
            }
        }
    }
    Ok(scope)
}

pub fn history_entry_to_res(entry: EntityHistoryEntry) -> EntityHistoryEntryRes {
    EntityHistoryEntryRes {
        entity: entity_to_res(entry.entity),
        changed_properties: entry
            .changed_properties
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        transaction_meta: entry.transaction_meta,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn transaction_req_roundtrip() {
        let req = TransactionReq {
            meta: {
                let mut m = serde_json::Map::new();
                m.insert("reasoning".into(), json!("test"));
                m
            },
            create: vec![EntityReq {
                slug: "alice".into(),
                description: Some(json!("A person")),
                properties: vec![crate::dto::request::PropertyValueReq {
                    property: "age".into(),
                    value: json!(25),
                }],
                meta: serde_json::Map::new(),
                status: None,
            }],
            update: vec![],
            create_managed: vec![],
            update_managed: vec![],
            delete: vec![],
            touch: vec![],
        };
        transaction_req_to_input(req).unwrap();
    }

    #[test]
    fn invalid_slug_rejected() {
        let req = TransactionReq {
            meta: serde_json::Map::new(),
            create: vec![EntityReq {
                slug: "INVALID SLUG!!!".into(),
                description: None,
                properties: vec![],
                meta: serde_json::Map::new(),
                status: None,
            }],
            update: vec![],
            create_managed: vec![],
            update_managed: vec![],
            delete: vec![],
            touch: vec![],
        };
        assert!(transaction_req_to_input(req).is_err());
    }

    #[test]
    fn checkout_req_converts() {
        let req = CheckoutReq {
            slug: "feature".into(),
            meta: {
                let mut m = serde_json::Map::new();
                m.insert("reasoning".into(), json!("explore"));
                m
            },
            created_from_tx: None,
            transaction: TransactionReq {
                meta: {
                    let mut m = serde_json::Map::new();
                    m.insert("reasoning".into(), json!("init"));
                    m
                },
                create: vec![],
                update: vec![],
                create_managed: vec![],
                update_managed: vec![],
                delete: vec![],
                touch: vec![],
            },
        };
        checkout_req_to_input(req).unwrap();
    }
}
