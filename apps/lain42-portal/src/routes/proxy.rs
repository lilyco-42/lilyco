//! 本地子服务反向代理（/mc-assets、/relay、/api/pool）。

use axum::body::{to_bytes, Body};
use axum::extract::{Path, Request};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(super) async fn proxy_pool_root(req: Request) -> Response {
    proxy_local_service(8793, "", req).await
}

pub(super) async fn proxy_pool_sub(Path(rest): Path<String>, req: Request) -> Response {
    proxy_local_service(8793, &rest, req).await
}

pub(super) async fn proxy_mc_assets_root(req: Request) -> Response {
    proxy_local_service(8088, "", req).await
}

pub(super) async fn proxy_mc_assets_sub(Path(rest): Path<String>, req: Request) -> Response {
    proxy_local_service(8088, &rest, req).await
}

pub(super) async fn proxy_relay_root(req: Request) -> Response {
    proxy_local_service(8090, "", req).await
}

pub(super) async fn proxy_relay_sub(Path(rest): Path<String>, req: Request) -> Response {
    proxy_local_service(8090, &rest, req).await
}

async fn proxy_local_service(port: u16, rest: &str, req: Request) -> Response {
    let method = req.method().as_str().to_string();
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_path = if rest.is_empty() {
        format!("/{query}")
    } else {
        format!("/{rest}{query}")
    };
    let req_headers = req.headers().clone();
    let body_bytes = match to_bytes(req.into_body(), 4 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("read body error: {e}")).into_response(),
    };

    let mut stream = match TcpStream::connect(format!("127.0.0.1:{port}")).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("后端服务 (127.0.0.1:{port}) 暂未就绪: {e}"),
            )
                .into_response()
        }
    };

    let mut wire = format!("{method} {target_path} HTTP/1.1\r\nHost: 127.0.0.1:8088\r\nConnection: close\r\nAccept-Encoding: identity\r\n");
    for (k, v) in &req_headers {
        let kn = k.as_str().to_ascii_lowercase();
        if matches!(kn.as_str(), "host" | "connection" | "accept-encoding" | "content-length" | "transfer-encoding") {
            continue;
        }
        if let Ok(vs) = v.to_str() {
            wire.push_str(&format!("{}: {}\r\n", k.as_str(), vs));
        }
    }
    if !body_bytes.is_empty() {
        wire.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    wire.push_str("\r\n");

    if stream.write_all(wire.as_bytes()).await.is_err()
        || (!body_bytes.is_empty() && stream.write_all(&body_bytes).await.is_err())
    {
        return (StatusCode::BAD_GATEWAY, "写入上游请求失败").into_response();
    }

    let mut raw = Vec::with_capacity(8192);
    if stream.read_to_end(&mut raw).await.is_err() {
        return (StatusCode::BAD_GATEWAY, "读取上游响应失败").into_response();
    }

    let Some(sep) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return (StatusCode::BAD_GATEWAY, "上游响应格式无效").into_response();
    };
    let head_str = String::from_utf8_lossy(&raw[..sep]);
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(502);
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut resp_builder = Response::builder().status(status);
    let mut is_html = false;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let kn = k.trim().to_ascii_lowercase();
            let vv = v.trim();
            if matches!(kn.as_str(), "content-length" | "transfer-encoding" | "connection") {
                continue;
            }
            if kn == "content-type" && vv.contains("text/html") {
                is_html = true;
            }
            if let (Ok(hn), Ok(hv)) = (
                HeaderName::from_bytes(kn.as_bytes()),
                HeaderValue::from_str(vv),
            ) {
                resp_builder = resp_builder.header(hn, hv);
            }
        }
    }

    let body_slice = &raw[sep + 4..];
    let final_body = if is_html {
        let html = String::from_utf8_lossy(body_slice)
            .replace("'/api/v1/", "'/mc-assets/api/v1/")
            .replace("`/api/v1/", "`/mc-assets/api/v1/");
        html.into_bytes()
    } else {
        body_slice.to_vec()
    };

    resp_builder
        .body(Body::from(final_body))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "response build error").into_response())
}
