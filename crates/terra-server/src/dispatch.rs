//! Command dispatch — routes a request envelope to the right executor via Terra.

use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use terra_core::command::input::get_branch::GetBranchQuery;
use terra_core::command::input::get_transaction::GetTransactionQuery;
use terra_core::command::input::list_managed::ListManagedQuery;
use terra_core::command::input::list_transactions::ListTransactionsQuery;
use terra_core::command::input::similar_entities::SimilarEntitiesQuery;
use terra_core::command::input::touched_entities::TouchedEntitiesQuery;
use terra_core::io::slug::Slug;
use terra_core::Terra;

use crate::dto::convert;
use crate::dto::request::{
    CheckoutReq, CommandEnvelope, EntityHistoryReq, GetTransactionReq, ListManagedReq,
    ListTransactionsReq, SimilarEntitiesReq, TouchedEntitiesReq, TransactionReq,
};
use crate::dto::response::ErrorRes;
use crate::error::classify;
use crate::format::ContentFormat;

/// Parse the request body, dispatch to the right command, return the response.
pub fn handle(terra: &Terra, body: &[u8], format: ContentFormat) -> Response {
    let envelope: CommandEnvelope = match format.deserialize(body) {
        Ok(v) => v,
        Err(e) => return error_response(format, StatusCode::BAD_REQUEST, "parse_error", &e),
    };

    let branch: Slug = match envelope.branch.parse() {
        Ok(s) => s,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "invalid_slug",
                &e.to_string(),
            )
        }
    };

    match envelope.command.as_str() {
        "transaction" => cmd_transaction(terra, &branch, envelope.body, format),
        "checkout" => cmd_checkout(terra, &branch, envelope.body, format),
        "transactions.list" => cmd_list_transactions(terra, &branch, envelope.body, format),
        "entities.touched" => cmd_touched_entities(terra, &branch, envelope.body, format),
        "branch.get" => cmd_get_branch(terra, &branch, format),
        "managed.list" => cmd_list_managed(terra, &branch, envelope.body, format),
        "transaction.get" => cmd_get_transaction(terra, &branch, envelope.body, format),
        "entities.similar" => cmd_similar_entities(terra, &branch, envelope.body, format),
        "entity.history" => cmd_entity_history(terra, &branch, envelope.body, format),
        other => error_response(
            format,
            StatusCode::BAD_REQUEST,
            "unknown_command",
            &format!("unknown command: {other}"),
        ),
    }
}

fn cmd_transaction(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: TransactionReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = match convert::transaction_req_to_input(req) {
        Ok(v) => v,
        Err(e) => return error_response(format, StatusCode::BAD_REQUEST, "parse_error", &e),
    };
    match terra.execute(branch, input) {
        Ok(tx) => ok_response(format, &convert::transaction_to_res(tx)),
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_checkout(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: CheckoutReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = match convert::checkout_req_to_input(req) {
        Ok(v) => v,
        Err(e) => return error_response(format, StatusCode::BAD_REQUEST, "parse_error", &e),
    };
    match terra.execute(branch, input) {
        Ok(out) => ok_response(format, &convert::checkout_to_res(out)),
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_list_transactions(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: ListTransactionsReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = ListTransactionsQuery::new(req.at_tx, req.limit);
    match terra.execute(branch, input) {
        Ok(txs) => {
            let res: Vec<_> = txs.into_iter().map(convert::transaction_to_res).collect();
            ok_response(format, &res)
        }
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_touched_entities(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: TouchedEntitiesReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = TouchedEntitiesQuery::new(req.at_tx, req.limit);
    match terra.execute(branch, input) {
        Ok(entities) => {
            let res: Vec<_> = entities.into_iter().map(convert::entity_to_res).collect();
            ok_response(format, &res)
        }
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_get_branch(terra: &Terra, branch: &Slug, format: ContentFormat) -> Response {
    match terra.execute(branch, GetBranchQuery::new()) {
        Ok(b) => ok_response(format, &convert::branch_to_res(b)),
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_list_managed(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: ListManagedReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = ListManagedQuery::new(req.at_tx);
    match terra.execute(branch, input) {
        Ok(items) => {
            let res: Vec<_> = items.into_iter().map(convert::managed_to_res).collect();
            ok_response(format, &res)
        }
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_get_transaction(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: GetTransactionReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = GetTransactionQuery::new(req.tx_id);
    match terra.execute(branch, input) {
        Ok(detail) => ok_response(format, &convert::transaction_detail_to_res(detail)),
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_similar_entities(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: SimilarEntitiesReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let mut input = SimilarEntitiesQuery::new(req.queries, req.limit, req.min_similarity);
    if let Some(tx) = req.at_tx {
        input = input.at_tx(tx);
    }
    match terra.execute(branch, input) {
        Ok(pairs) => ok_response(format, &convert::similar_to_res(pairs)),
        Err(e) => db_error_response(format, &e),
    }
}

fn cmd_entity_history(
    terra: &Terra,
    branch: &Slug,
    body: serde_json::Value,
    format: ContentFormat,
) -> Response {
    let req: EntityHistoryReq = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                format,
                StatusCode::BAD_REQUEST,
                "parse_error",
                &e.to_string(),
            )
        }
    };
    let input = match convert::entity_history_req_to_query(req) {
        Ok(v) => v,
        Err(e) => return error_response(format, StatusCode::BAD_REQUEST, "parse_error", &e),
    };
    match terra.execute(branch, input) {
        Ok(entries) => {
            let res: Vec<_> = entries
                .into_iter()
                .map(convert::history_entry_to_res)
                .collect();
            ok_response(format, &res)
        }
        Err(e) => db_error_response(format, &e),
    }
}

// --- Helpers ---

fn ok_response(format: ContentFormat, body: &impl serde::Serialize) -> Response {
    match format.serialize(body) {
        Ok(bytes) => (
            StatusCode::OK,
            [(CONTENT_TYPE, format.content_type())],
            bytes,
        )
            .into_response(),
        Err(e) => error_response(
            format,
            StatusCode::INTERNAL_SERVER_ERROR,
            "serialize_error",
            &e,
        ),
    }
}

fn db_error_response(format: ContentFormat, err: &terra_core::io::DbError) -> Response {
    let (status, kind) = classify(err);
    error_response(format, status, kind, &err.to_string())
}

fn error_response(
    format: ContentFormat,
    status: StatusCode,
    kind: &str,
    message: &str,
) -> Response {
    let body = ErrorRes {
        error: message.to_string(),
        kind: kind.to_string(),
    };
    let bytes = format
        .serialize(&body)
        .unwrap_or_else(|_| serde_json::to_vec(&body).unwrap_or_default());
    (status, [(CONTENT_TYPE, format.content_type())], bytes).into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indoc::indoc;
    use serde_json::json;
    use terra_core::config::{DataSchema, ProjectConfig};
    use terra_core::embed::NoopEmbedder;

    use super::*;

    fn open_terra(dir: &std::path::Path) -> Terra {
        let config = Arc::new(
            ProjectConfig::builder()
                .data_dir("./data".into())
                .schema_path("./schema.yaml".into())
                .build(),
        );
        let schema = Arc::new(
            DataSchema::from_yaml(indoc! {"
                transaction_meta:
                  reasoning:
                    type: text
                    required: true
                entity_change_meta:
                  reasoning:
                    type: text
                    required: true
                branch_meta:
                  reasoning:
                    type: text
                    required: true
                managed_types:
                  task:
                    fields:
                      goal: { type: json, required: true }
                    lifecycle:
                      initial: open
                      visible: [open]
            "})
            .unwrap(),
        );
        Terra::open(dir, config, schema, Arc::new(NoopEmbedder)).unwrap()
    }

    fn dispatch_json(terra: &Terra, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let bytes = serde_json::to_vec(&body).unwrap();
        let response = handle(terra, &bytes, ContentFormat::Json);
        let status = response.status();
        let body_bytes = tokio::runtime::Runtime::new().unwrap().block_on(async {
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
        });
        let value: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        (status, value)
    }

    #[test]
    fn transaction_creates_entity() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "branch": "main",
                "meta": { "reasoning": "create entity" },
                "create": [{
                    "slug": "alice",
                    "description": "A person",
                    "properties": [{ "property": "age", "value": 25 }],
                    "meta": { "reasoning": "initial" }
                }]
            }),
        );

        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["meta"]["reasoning"], "create entity");
        assert!(res["context"]["tx_id"].is_string());
        assert_eq!(res["context"]["branch"], "main");
    }

    #[test]
    fn list_transactions_returns_recent() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "first" }
            }),
        );
        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "second" }
            }),
        );

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "transactions.list",
                "limit": 10
            }),
        );

        assert_eq!(status, StatusCode::OK);
        let arr = res.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["meta"]["reasoning"], "second");
        assert_eq!(arr[1]["meta"]["reasoning"], "first");
    }

    #[test]
    fn touched_entities_after_create() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "create" },
                "create": [{
                    "slug": "bob",
                    "description": "B person",
                    "properties": [{ "property": "city", "value": "Berlin" }],
                    "meta": { "reasoning": "initial" }
                }]
            }),
        );

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "entities.touched",
                "limit": 10
            }),
        );

        assert_eq!(status, StatusCode::OK);
        let arr = res.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["slug"], "bob");
        assert_eq!(arr[0]["properties"][0]["property"], "city");
        assert_eq!(arr[0]["properties"][0]["value"], "Berlin");
    }

    #[test]
    fn checkout_creates_branch() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "seed" }
            }),
        );

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "checkout",
                "branch": "main",
                "slug": "feature",
                "meta": { "reasoning": "explore" },
                "transaction": {
                    "meta": { "reasoning": "first on branch" }
                }
            }),
        );

        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["branch"], "feature");
        assert!(res["created_from_tx"].is_string());

        let (status, branch) = dispatch_json(
            &terra,
            json!({
                "command": "branch.get",
                "branch": "feature"
            }),
        );
        assert_eq!(status, StatusCode::OK);
        assert_eq!(branch["slug"], "feature");
        assert_eq!(branch["parent"], "main");
    }

    #[test]
    fn get_branch_main() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "branch.get"
            }),
        );

        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["slug"], "main");
        assert_eq!(res["parent"], "main");
    }

    #[test]
    fn unknown_command_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "nonexistent"
            }),
        );

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(res["kind"], "unknown_command");
    }

    #[test]
    fn invalid_slug_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "branch": "INVALID SLUG!!!",
                "meta": { "reasoning": "test" }
            }),
        );

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(res["kind"], "invalid_slug");
    }

    #[test]
    fn validation_error_returns_400() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": {}
            }),
        );

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(res["kind"], "validation_error");
    }

    #[test]
    fn default_branch_is_main() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "branch.get"
            }),
        );

        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["slug"], "main");
    }

    #[test]
    fn transaction_get_latest() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "create" },
                "create": [{
                    "slug": "alice",
                    "description": "A person",
                    "properties": [{ "property": "age", "value": 25 }],
                    "meta": { "reasoning": "initial" }
                }]
            }),
        );

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "transaction.get"
            }),
        );

        assert_eq!(status, StatusCode::OK);
        assert_eq!(res["meta"]["reasoning"], "create");
        assert_eq!(res["branch"], "main");
        assert_eq!(res["created"][0]["slug"], "alice");
        assert_eq!(res["created"][0]["properties"][0]["property"], "age");
        assert_eq!(res["created"][0]["properties"][0]["value"], 25);
        assert!(res["context"]["tx_id"].is_string());
    }

    #[test]
    fn entity_history_returns_entries() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        // Create alice with age=25.
        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "create alice" },
                "create": [{
                    "slug": "alice",
                    "description": "a person",
                    "properties": [{ "property": "age", "value": 25 }],
                    "meta": { "reasoning": "initial" }
                }]
            }),
        );

        // Update alice: age=26, city=Berlin.
        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "alice update" },
                "update": [{
                    "slug": "alice",
                    "properties": [
                        { "property": "age", "value": 26 },
                        { "property": "city", "value": "Berlin" }
                    ],
                    "meta": { "reasoning": "birthday and move" }
                }]
            }),
        );

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "entity.history",
                "entity": "alice",
                "limit": 10
            }),
        );

        assert_eq!(status, StatusCode::OK);
        let arr = res.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        // Most recent first.
        assert_eq!(arr[0]["transaction_meta"]["reasoning"], "alice update");
        let changed0: Vec<&str> = arr[0]["changed_properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(changed0.contains(&"age"));
        assert!(changed0.contains(&"city"));

        assert_eq!(arr[1]["transaction_meta"]["reasoning"], "create alice");
    }

    #[test]
    fn entity_history_filter_by_property() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "create" },
                "create": [{
                    "slug": "alice",
                    "description": "p",
                    "properties": [{ "property": "age", "value": 25 }],
                    "meta": { "reasoning": "initial" }
                }]
            }),
        );
        dispatch_json(
            &terra,
            json!({
                "command": "transaction",
                "meta": { "reasoning": "city only" },
                "update": [{
                    "slug": "alice",
                    "properties": [{ "property": "city", "value": "Berlin" }],
                    "meta": { "reasoning": "moved" }
                }]
            }),
        );

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "entity.history",
                "entity": "alice",
                "property": "city",
                "limit": 10
            }),
        );

        assert_eq!(status, StatusCode::OK);
        let arr = res.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["transaction_meta"]["reasoning"], "city only");
    }

    #[test]
    fn entity_history_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let (status, res) = dispatch_json(
            &terra,
            json!({
                "command": "entity.history",
                "entity": "ghost",
                "limit": 10
            }),
        );

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(res["kind"], "not_found");
    }

    #[test]
    fn yaml_format_works() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());

        let yaml_body = indoc! {"
            command: branch.get
            branch: main
        "};

        let response = handle(&terra, yaml_body.as_bytes(), ContentFormat::Yaml);
        assert_eq!(response.status(), StatusCode::OK);
    }
}
