//! 数据源抽象：宿主应用把“新弹幕”喂给悬浮层，不需要关心 GTK。

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct FloatItem {
    pub ts: i64,
    pub text: String,
}

/// 悬浮层的数据源。`poll` 会被后台线程按 `interval_ms` 周期调用。
pub trait Feed: Send + 'static {
    fn poll(&mut self) -> Vec<FloatItem>;

    fn interval_ms(&self) -> u64 {
        1000
    }
}

/// 宿主应用自己推送（channel）。
pub struct ChannelFeed {
    rx: Receiver<FloatItem>,
}

impl ChannelFeed {
    pub fn pair() -> (Sender<FloatItem>, Self) {
        let (tx, rx) = mpsc::channel();
        (tx, Self { rx })
    }
}

impl Feed for ChannelFeed {
    fn poll(&mut self) -> Vec<FloatItem> {
        let mut items = Vec::new();
        while let Ok(item) = self.rx.try_recv() {
            items.push(item);
        }
        items
    }

    fn interval_ms(&self) -> u64 {
        200
    }
}

#[cfg(feature = "http")]
/// 轮询任意返回 JSON 的接口，按“数组字段 + ts/text 字段”取新条目。
///
/// 例如 lyblive：`{"recent":[{"ts":1795000000,"text":"hi"}]}`
#[cfg(feature = "http")]
pub struct HttpJsonFeed {
    url: String,
    array_field: String,
    ts_field: String,
    text_field: String,
    last_ts: i64,
    client: reqwest::blocking::Client,
    interval_ms: u64,
}

#[cfg(feature = "http")]
impl HttpJsonFeed {
    pub fn new(
        url: impl Into<String>,
        array_field: impl Into<String>,
        ts_field: impl Into<String>,
        text_field: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            array_field: array_field.into(),
            ts_field: ts_field.into(),
            text_field: text_field.into(),
            last_ts: 0,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new()),
            interval_ms: 1000,
        }
    }

    pub fn interval(mut self, ms: u64) -> Self {
        self.interval_ms = ms.max(200);
        self
    }
}

#[cfg(feature = "http")]
impl Feed for HttpJsonFeed {
    fn poll(&mut self) -> Vec<FloatItem> {
        // 阻塞式轮询放在后台线程里，不占用 GTK 主线程
        thread::sleep(Duration::from_millis(self.interval_ms));
        let Ok(resp) = self.client.get(&self.url).send() else {
            return Vec::new();
        };
        let Ok(body) = resp.json::<serde_json::Value>() else {
            return Vec::new();
        };
        let Some(items) = body.get(&self.array_field).and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in items {
            let ts = item.get(&self.ts_field).and_then(|v| v.as_i64()).unwrap_or(0);
            let text = item
                .get(&self.text_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if ts <= self.last_ts || text.is_empty() {
                continue;
            }
            self.last_ts = ts;
            out.push(FloatItem { ts, text });
        }
        out
    }

    fn interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_feed_drains_pushes() {
        let (tx, mut feed) = ChannelFeed::pair();
        tx.send(FloatItem { ts: 1, text: "a".into() }).unwrap();
        tx.send(FloatItem { ts: 2, text: "b".into() }).unwrap();
        let items = feed.poll();
        assert_eq!(items.len(), 2);
        assert!(feed.poll().is_empty());
    }

    #[test]
    fn channel_feed_is_fast() {
        let (_tx, feed) = ChannelFeed::pair();
        assert!(feed.interval_ms() <= 500);
    }
}