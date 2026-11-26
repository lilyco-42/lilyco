//! Ship the Web renderer's editable TOML catalog in the offline console.

use std::collections::BTreeMap;
use std::sync::OnceLock;

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

pub(crate) fn script_catalog() -> &'static str {
    static JSON: OnceLock<String> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::to_string(catalog())
            .unwrap_or_else(|_| "{}".into())
            .replace('<', "\\u003c")
    })
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
}
