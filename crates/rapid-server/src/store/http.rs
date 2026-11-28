//! Shared HTTP transport for database connections.

use std::{sync::Arc, time::Duration};

use reqwest::{Client, header::HeaderValue};
use url::{Host, Url};

use super::{StoreError, environment};

pub(super) fn client(timeout: Duration) -> Result<Client, StoreError> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| failure("connect", "TLS unavailable"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Client::builder()
        .tls_backend_preconfigured(tls)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .timeout(timeout)
        .build()
        .map_err(|_| failure("connect", "HTTP client unavailable"))
}

pub(super) fn authorization(key: &str, local: bool) -> Result<Option<HeaderValue>, StoreError> {
    let value = match environment(key) {
        Ok(value) if value.bytes().all(|byte| byte.is_ascii_graphic()) => value,
        Ok(_) => return Err(StoreError::InvalidToken(key.to_owned())),
        Err(StoreError::MissingVariable(_)) if local => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut auth = HeaderValue::from_str(&format!("Bearer {value}"))
        .map_err(|_| StoreError::InvalidToken(key.to_owned()))?;
    auth.set_sensitive(true);
    Ok(Some(auth))
}

pub(super) fn endpoint(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    match (url.scheme(), url.host()?) {
        ("https", _) => {}
        ("http", Host::Ipv4(ip)) if ip.is_loopback() => {}
        ("http", Host::Ipv6(ip)) if ip.is_loopback() => {}
        ("http", Host::Domain("localhost")) => {}
        _ => return None,
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url)
}

pub(super) fn failure(operation: &'static str, kind: &'static str) -> StoreError {
    StoreError::Remote { operation, kind }
}
