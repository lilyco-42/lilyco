//! Utoipa owns the OpenAPI document. Schemars derives inline schemas from
//! the resource's Serde model, including `deny_unknown_fields`.

use schemars::{JsonSchema, generate::SchemaSettings};
use utoipa::openapi::{
    Content, Info, OpenApi, RefOr, Required, Response,
    path::{
        HttpMethod, OperationBuilder, Parameter, ParameterBuilder, ParameterIn, PathItem,
        PathItemBuilder, Paths,
    },
    request_body::{RequestBody, RequestBodyBuilder},
    response::ResponseBuilder,
    schema::Schema,
};

use crate::{
    error::Problem,
    store::{Page, Record},
};

#[derive(Default)]
pub struct Routes(Paths);

impl Routes {
    pub fn resource<T: JsonSchema>(&mut self, name: &str) -> Result<(), serde_json::Error> {
        let docs = ResourceDoc::for_type::<T>()?;
        let id = parameter("id", ParameterIn::Path, docs.cursor.clone());
        let after = parameter("after", ParameterIn::Query, docs.cursor.clone());
        let missing = docs.problem("Record not found");
        let id_error = docs.problem("Invalid record ID");

        let list = docs
            .operation(format!("list_{name}"))
            .parameter(after)
            .response("200", response("Records", docs.page.clone()))
            .response("400", docs.problem("Invalid cursor"));
        let create = docs
            .write(format!("create_{name}"))
            .response("201", response("Record created", docs.record.clone()))
            .response("400", docs.problem("Malformed JSON"));
        let read = docs
            .operation(format!("read_{name}"))
            .response("200", response("Record", docs.record.clone()))
            .response("404", missing.clone())
            .response("400", id_error.clone());
        let replace = docs
            .write(format!("replace_{name}"))
            .response("200", response("Record replaced", docs.record.clone()))
            .response("404", missing.clone())
            .response("400", docs.problem("Malformed JSON or invalid record ID"));
        let delete = docs
            .operation(format!("delete_{name}"))
            .response("204", Response::new("Record deleted"))
            .response("404", missing)
            .response("400", id_error);

        let path = format!("/api/{name}");
        self.0.paths.insert(
            path.clone(),
            PathItemBuilder::new()
                .operation(HttpMethod::Get, list)
                .operation(HttpMethod::Post, create)
                .build(),
        );
        self.0.paths.insert(
            format!("{path}/{{id}}"),
            PathItemBuilder::new()
                .parameters(Some([id]))
                .operation(HttpMethod::Get, read)
                .operation(HttpMethod::Put, replace)
                .operation(HttpMethod::Delete, delete)
                .build(),
        );
        Ok(())
    }

    pub fn document(mut self, title: &str) -> Result<OpenApi, serde_json::Error> {
        let health = OperationBuilder::new()
            .operation_id(Some("health"))
            .response("204", Response::new("API is running"));
        let ready = OperationBuilder::new()
            .operation_id(Some("ready"))
            .response("204", Response::new("Database is ready"));
        let ready = storage_errors(ready, &schema::<Problem>()?);
        self.0
            .paths
            .insert("/healthz".into(), PathItem::new(HttpMethod::Get, health));
        self.0
            .paths
            .insert("/readyz".into(), PathItem::new(HttpMethod::Get, ready));
        Ok(OpenApi::new(
            Info::new(title, env!("CARGO_PKG_VERSION")),
            self.0,
        ))
    }
}

// Transport metadata combines one schema per response shape and shared parameters.
struct ResourceDoc {
    payload: RequestBody,
    record: Schema,
    page: Schema,
    cursor: Schema,
    problem: Schema,
}

impl ResourceDoc {
    fn for_type<T: JsonSchema>() -> Result<Self, serde_json::Error> {
        Ok(Self {
            payload: RequestBodyBuilder::new()
                .required(Some(Required::True))
                .content("application/json", Content::new(Some(schema::<T>()?)))
                .build(),
            record: schema::<Record<T>>()?,
            page: schema::<Page<T>>()?,
            cursor: schema::<u32>()?,
            problem: schema::<Problem>()?,
        })
    }

    fn operation(&self, id: String) -> OperationBuilder {
        storage_errors(
            OperationBuilder::new().operation_id(Some(id)),
            &self.problem,
        )
    }

    fn problem(&self, description: &str) -> Response {
        response(description, self.problem.clone())
    }

    fn write(&self, id: String) -> OperationBuilder {
        self.operation(id)
            .request_body(Some(self.payload.clone()))
            .response("413", self.problem("Body is too large"))
            .response("415", self.problem("Content type is not JSON"))
            .response(
                "422",
                self.problem("Body does not match the resource or failed validation"),
            )
    }
}

fn schema<T: JsonSchema>() -> Result<Schema, serde_json::Error> {
    let settings = SchemaSettings::draft2020_12().with(|setting| {
        setting.inline_subschemas = true;
        setting.meta_schema = None;
    });
    serde_json::from_value(
        settings
            .into_generator()
            .into_root_schema_for::<T>()
            .to_value(),
    )
}

fn parameter(name: &str, location: ParameterIn, schema: Schema) -> Parameter {
    let required = if location == ParameterIn::Path {
        Required::True
    } else {
        Required::False
    };
    ParameterBuilder::new()
        .name(name)
        .parameter_in(location)
        .required(required)
        .schema(Some(RefOr::T(schema)))
        .build()
}

fn response(description: &str, schema: Schema) -> Response {
    ResponseBuilder::new()
        .description(description)
        .content("application/json", Content::new(Some(schema)))
        .build()
}

fn storage_errors(operation: OperationBuilder, problem: &Schema) -> OperationBuilder {
    operation
        .response("500", response("Storage operation failed", problem.clone()))
        .response("503", response("Database is busy", problem.clone()))
}
