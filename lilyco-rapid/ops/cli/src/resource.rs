use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{Result, slug};

enum Kind {
    String,
    Bool,
    I32,
}

struct Field<'a> {
    name: &'a str,
    kind: Kind,
}

pub fn write(project: &Path, collection: &str, input: &[String]) -> Result<PathBuf> {
    if !project.join("Rapid.toml").is_file() {
        return Err("project directory must contain Rapid.toml".into());
    }
    if !slug(collection) {
        return Err(
            "collection must be a lowercase, hyphenated name of at most 48 characters".into(),
        );
    }
    let fields = input
        .iter()
        .map(|value| parse(value))
        .collect::<Result<Vec<_>>>()?;
    let unique: BTreeSet<_> = fields.iter().map(|field| field.name).collect();
    if fields.is_empty() || unique.len() != fields.len() {
        return Err("provide at least one field, with unique names".into());
    }
    let resources = project.join("core/resources");
    let path = resources.join(format!("{}.rs", collection.replace('-', "_")));
    let mut generated = tempfile::NamedTempFile::new_in(&resources)?;
    generated.write_all(model(collection, &fields).as_bytes())?;
    generated.persist_noclobber(&path)?;
    Ok(path)
}

fn parse(spec: &str) -> Result<Field<'_>> {
    let (name, value) = spec.split_once(':').ok_or("expected field name:type")?;
    if name.len() > 48
        || !name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || matches!(name, "self" | "super" | "crate")
    {
        return Err(format!("invalid field name {name:?}").into());
    }
    let kind = match value {
        "string" => Kind::String,
        "bool" => Kind::Bool,
        "i32" => Kind::I32,
        _ => {
            return Err(
                format!("unsupported field type {value:?}, use string, bool, or i32").into(),
            );
        }
    };
    Ok(Field { name, kind })
}

fn model(collection: &str, fields: &[Field<'_>]) -> String {
    let has_text = fields
        .iter()
        .any(|field| matches!(field.kind, Kind::String));
    let mut output = String::new();
    if has_text {
        output.push_str("use alloc::string::String;\n");
    }
    output.push_str("use core::num::NonZeroU16;\nuse serde::{Deserialize, Serialize};\n\n");
    output.push_str(if has_text {
        "use crate::{FieldError, Resource, validate_text};\n"
    } else {
        "use crate::{FieldError, Resource};\n"
    });
    output.push_str("\n#[derive(Debug, Deserialize, Serialize)]\n#[cfg_attr(feature = \"schema\", derive(schemars::JsonSchema))]\n#[serde(deny_unknown_fields)]\n");
    if fields.len() > 4 {
        output.push_str("// The wire DTO mirrors the generated resource fields.\n");
    }
    output.push_str("pub struct Model {\n");
    for field in fields {
        let ty = match field.kind {
            Kind::String => "String",
            Kind::Bool => "bool",
            Kind::I32 => "i32",
        };
        output.push_str(&format!("    pub r#{}: {ty},\n", field.name));
    }
    output.push_str(&format!(
        "}}\n\nimpl Resource for Model {{\n    const COLLECTION: &'static str = {collection:?};\n\n"
    ));
    let limit = if has_text {
        "max_text_bytes"
    } else {
        "_max_text_bytes"
    };
    output.push_str(&format!(
        "    fn validate(&self, {limit}: NonZeroU16) -> Result<(), FieldError> {{\n"
    ));
    for field in fields
        .iter()
        .filter(|field| matches!(field.kind, Kind::String))
    {
        let name = field.name;
        output.push_str(&format!(
            "        validate_text({name:?}, &self.r#{name}, max_text_bytes)?;\n"
        ));
    }
    output.push_str("        Ok(())\n    }\n}\n");
    output
}
