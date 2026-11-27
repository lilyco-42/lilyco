//! Ship the Web renderer's editable TOML catalog in the offline console.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use lilyco_core::schema::CommandSchema;
use serde::Deserialize;

type Messages = BTreeMap<String, String>;
type Catalog = BTreeMap<String, Messages>;
const TRANSLATIONS: &str = include_str!("../assets/i18n/ui.toml");

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str(TRANSLATIONS).unwrap_or_else(|error| {
            eprintln!("Lilyco GUI: invalid built-in translation catalog: {error}");
            Catalog::new()
        })
    })
}

pub(crate) fn default_message(key: &str) -> &str {
    catalog()
        .get("zh-CN")
        .and_then(|entries| entries.get(key))
        .map_or(key, String::as_str)
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct CommandText {
    about: Option<String>,
    args: Messages,
}

pub(crate) fn script_catalog<'a>(
    commands: impl IntoIterator<Item = (Option<&'a str>, &'a CommandSchema, Option<&'static str>)>,
) -> String {
    let mut messages = catalog().clone();
    for (name, schema, source) in commands {
        let localized: BTreeMap<String, CommandText> = match source {
            Some(toml) => toml::from_str(toml).unwrap_or_else(|error| {
                eprintln!(
                    "Lilyco GUI: invalid language catalog for {}: {error}",
                    schema.name
                );
                BTreeMap::new()
            }),
            None => BTreeMap::new(),
        };
        let key = match name {
            Some(name) => format!("app.{name}"),
            None => "app".to_owned(),
        };
        for (locale, entries) in &mut messages {
            let text = localized.get(locale);
            let about = text
                .and_then(|m| m.about.as_deref())
                .unwrap_or(&schema.about);
            entries.insert(format!("{key}.about"), about.to_owned());
            for arg in &schema.args {
                let label = text
                    .and_then(|m| m.args.get(&arg.name).map(String::as_str))
                    .filter(|localized| !localized.trim().is_empty())
                    .unwrap_or_else(|| {
                        if arg.about.is_empty() {
                            &arg.name
                        } else {
                            &arg.about
                        }
                    });
                entries.insert(format!("{key}.arg.{}", arg.name), label.to_owned());
            }
        }
    }
    serde_json::to_string(&messages)
        .unwrap_or_else(|_| "{}".into())
        // Keep app messages inside the script and protect template markers in their text.
        .replace('<', "\\u003c")
        .replace("__", "\\u005f\\u005f")
}

/// Expand only the trusted template before schema or other dynamic data is inserted.
pub(crate) fn localize_template(template: &str) -> String {
    let mut html = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((prefix, token)) = remaining.split_once("@{") {
        let Some((key, suffix)) = token.split_once('}') else {
            break;
        };
        html.push_str(prefix);
        html.push_str(&crate::util::html_escape(default_message(key)));
        remaining = suffix;
    }
    html.push_str(remaining);
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_cover_the_same_messages_and_format_parameters() {
        let translations: Catalog = toml::from_str(TRANSLATIONS).expect("valid TOML");
        let chinese = &translations["zh-CN"];
        let english = &translations["en"];
        assert_eq!(
            chinese.keys().collect::<Vec<_>>(),
            english.keys().collect::<Vec<_>>()
        );
        for (key, text) in chinese {
            assert!(!text.is_empty() && !english[key].is_empty(), "{key}");
            let parameters = |message: &str| {
                let mut names: Vec<String> = message
                    .split('{')
                    .skip(1)
                    .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_owned()))
                    .collect();
                names.sort();
                names
            };
            assert_eq!(parameters(text), parameters(&english[key]), "{key}");
        }
    }

    #[test]
    fn application_catalog_overrides_descriptions_only_for_the_configured_locale() {
        let schema = crate::state::fixture::schema_of(
            "internal",
            "Default about",
            vec![crate::state::fixture::arg(
                "output",
                "Output path",
                lilyco_core::schema::ArgKind::Text,
                false,
                None,
            )],
        );
        let source = r#"
[zh-CN]
about = "图片工具 __META__"
[zh-CN.args]
output = "输出路径 </script>"
"#;
        let embedded = script_catalog([(Some("image"), &schema, Some(source))]);
        assert!(!embedded.contains("</script>") && !embedded.contains("__META__"));
        let parsed: Catalog = serde_json::from_str(&embedded).unwrap();
        assert_eq!(parsed["zh-CN"]["app.image.about"], "图片工具 __META__");
        assert_eq!(
            parsed["zh-CN"]["app.image.arg.output"],
            "输出路径 </script>"
        );
        assert_eq!(parsed["en"]["app.image.arg.output"], "Output path");
    }
}
