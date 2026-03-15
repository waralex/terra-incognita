//! Conversions between DTOs and terra-core-v0_2 domain types.

use terra_core_v0_2::command::executor::checkout::CheckoutOutput;
use terra_core_v0_2::command::input::checkout::CheckoutInput;
use terra_core_v0_2::command::input::transaction::{TouchItem, TransactionInput};
use terra_core_v0_2::domain::branch::Branch;
use terra_core_v0_2::domain::entity::Entity;
use terra_core_v0_2::domain::entity::PropertyValue;
use terra_core_v0_2::domain::managed::Managed;
use terra_core_v0_2::domain::transaction::Transaction;
use terra_core_v0_2::domain::tx_meta::TxMeta;
use terra_core_v0_2::io::slug::Slug;

use crate::dto::request::{
    CheckoutReq, EntityReq, ManagedReq, TransactionReq,
};
use crate::dto::response::{
    BranchRes, CheckoutRes, EntityRes, ManagedRes, PropertyValueRes,
    SimilarEntityRes, TransactionRes, TxMetaRes,
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
    for t in req.touch {
        input = input.touch(TouchItem::new(parse_slug(&t.entity)?, t.reasoning));
    }
    Ok(input)
}

pub fn checkout_req_to_input(req: CheckoutReq) -> Result<CheckoutInput, String> {
    let slug = parse_slug(&req.slug)?;
    let transaction = transaction_req_to_input(req.transaction)?;
    Ok(CheckoutInput::new(slug, req.meta, req.created_from_tx, transaction))
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
    Ok(Entity::new(slug, req.description, props, req.meta))
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

pub fn checkout_to_res(out: CheckoutOutput) -> CheckoutRes {
    CheckoutRes {
        branch: out.branch.to_string(),
        created_from_tx: out.created_from_tx,
        transaction: transaction_to_res(out.transaction),
    }
}

pub fn similar_to_res(pairs: Vec<(Slug, f32)>) -> Vec<SimilarEntityRes> {
    pairs
        .into_iter()
        .map(|(slug, similarity)| SimilarEntityRes {
            slug: slug.to_string(),
            similarity,
        })
        .collect()
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
            }],
            update: vec![],
            create_managed: vec![],
            update_managed: vec![],
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
            }],
            update: vec![],
            create_managed: vec![],
            update_managed: vec![],
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
                touch: vec![],
            },
        };
        checkout_req_to_input(req).unwrap();
    }
}
