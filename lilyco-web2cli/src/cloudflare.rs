use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CloudflareInterruption {
    Challenge,
    Block,
}

impl CloudflareInterruption {
    pub(crate) fn detect<'a>(
        status: u16,
        body: &str,
        header: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Self> {
        if header("cf-mitigated")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("challenge"))
        {
            return Some(Self::Challenge);
        }

        let cloudflare_edge = header("cf-ray").is_some()
            || header("server")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("cloudflare"));
        if status == 403
            && cloudflare_edge
            && body.contains("cf-error-details")
            && body.contains("Attention Required! | Cloudflare")
        {
            return Some(Self::Block);
        }
        None
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Challenge => "cloudflare_challenge",
            Self::Block => "cloudflare_block",
        }
    }
}

pub(crate) fn skip_har_entry(path: &str, entry: &Value) -> bool {
    if path.starts_with("/cdn-cgi/") {
        return true;
    }
    let response = &entry["response"];
    let status = response["status"]
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or_default();
    let body = response
        .pointer("/content/text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let header = |name: &str| {
        response["headers"].as_array()?.iter().find_map(|item| {
            let key = item["name"].as_str()?;
            let value = item["value"].as_str()?;
            key.eq_ignore_ascii_case(name).then_some(value)
        })
    };
    CloudflareInterruption::detect(status, body, header).is_some()
}
