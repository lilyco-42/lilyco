//! 端口分配：按“段”给本机应用分配回环端口，避免跨应用抢占。
//!
//! 规则（详见 `docs/PORT-ALLOCATION.md`）：
//! - 只分配回环端口；段内从首选端口线性 +1 探测，最多 20 次，不跨段借用；
//! - 段被外部程序占用的端口视为“不可用”，绝不抢占；
//! - 应用未声明首选端口时，从所属段起点开始探测。

use serde::Serialize;
use std::net::TcpListener;

pub struct PortBand {
    pub key: &'static str,
    pub label: &'static str,
    pub start: u16,
    pub end: u16,
}

/// 段划分：编号按用途分组（4200 金融、4300 游戏…），与 `docs/PORT-ALLOCATION.md` 保持一致。
pub const BANDS: [PortBand; 11] = [
    PortBand {
        key: "finance",
        label: "重要应用 / 金融 / 银行 / 支付",
        start: 4200,
        end: 4299,
    },
    PortBand {
        key: "game",
        label: "游戏",
        start: 4300,
        end: 4399,
    },
    PortBand {
        key: "media",
        label: "媒体创作（图像 / 音视频）",
        start: 4400,
        end: 4499,
    },
    PortBand {
        key: "office",
        label: "生产力 / 办公",
        start: 4500,
        end: 4599,
    },
    PortBand {
        key: "social",
        label: "社交 / 通讯",
        start: 4600,
        end: 4699,
    },
    PortBand {
        key: "commerce",
        label: "商业 / 计费",
        start: 4700,
        end: 4799,
    },
    PortBand {
        key: "ai",
        label: "AI / Agent 服务",
        start: 4800,
        end: 4899,
    },
    PortBand {
        key: "devtools",
        label: "开发工具 / 构建",
        start: 4900,
        end: 4999,
    },
    PortBand {
        key: "ops",
        label: "运维 / 管理面板",
        start: 8000,
        end: 8099,
    },
    PortBand {
        key: "livestream",
        label: "本机直播 / 媒体工具链",
        start: 8700,
        end: 8799,
    },
    PortBand {
        key: "internal",
        label: "lilyco 内部微服务",
        start: 57890,
        end: 57899,
    },
];

const MAX_PROBE: u16 = 20;
const FORBIDDEN_BELOW: u16 = 1024;

pub fn band_by_key(key: &str) -> Option<&'static PortBand> {
    BANDS.iter().find(|band| band.key.eq_ignore_ascii_case(key))
}

/// 按应用 id 推断所属段；显式 `hint`（manifest 的 `port_band`）优先。
pub fn band_for_app(id: &str, hint: Option<&str>) -> &'static PortBand {
    if let Some(key) = hint {
        if let Some(band) = band_by_key(key) {
            return band;
        }
    }
    let lower = id.to_ascii_lowercase();
    let pick = |needles: &[&str], key: &str| {
        needles
            .iter()
            .any(|n| lower.contains(n))
            .then(|| band_by_key(key))
            .flatten()
    };
    pick(&["bank", "pay", "finance", "wallet", "invoice"], "finance")
        .or_else(|| pick(&["game", "emulator", "steam"], "game"))
        .or_else(|| {
            pick(
                &[
                    "rembg", "vision", "asset", "photo", "video", "audio", "music", "danmu",
                ],
                "media",
            )
        })
        .or_else(|| pick(&["office", "doc", "note", "task", "calendar"], "office"))
        .or_else(|| pick(&["chat", "social", "im-", "mail"], "social"))
        .or_else(|| pick(&["billing", "shop", "commerce", "order"], "commerce"))
        .or_else(|| pick(&["ai", "agent", "mcp", "llm", "model", "harness"], "ai"))
        .or_else(|| {
            pick(
                &["lyblive", "lycomc", "lycodl", "live", "stream", "obs"],
                "livestream",
            )
        })
        .or_else(|| pick(&["panel", "proxy", "gateway", "hub", "ops-"], "ops"))
        .or_else(|| pick(&["lilyco"], "internal"))
        .unwrap_or_else(|| band_by_key("devtools").expect("devtools band must exist"))
}

/// 端口是否可绑定到回环（TIME_WAIT 之外的占用会被拒绝）。
pub fn is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub struct Allocation {
    pub port: u16,
    /// 首选端口被占、发生了段内漂移
    pub drifted: bool,
}

/// 在段内分配端口：从 `preferred` 起线性探测，最多 20 个；不跨段。
pub fn allocate(band: &PortBand, preferred: Option<u16>) -> Option<Allocation> {
    if band.start <= FORBIDDEN_BELOW || band.end < band.start {
        return None;
    }
    let start = match preferred {
        Some(port) if port >= band.start && port <= band.end => port,
        Some(_) => band.start,
        None => band.start,
    };

    let mut port = start;
    for _ in 0..=MAX_PROBE {
        if port > band.end {
            break;
        }
        if is_free(port) {
            return Some(Allocation {
                port,
                drifted: Some(port) != preferred && preferred.is_some(),
            });
        }
        port = port.saturating_add(1);
    }
    None
}

#[derive(Serialize)]
pub struct BandUsage {
    pub key: &'static str,
    pub label: &'static str,
    pub start: u16,
    pub end: u16,
    pub used: Vec<u16>,
    pub free: usize,
}

/// 各段当前占用快照（供 `GET /api/ports` 与面板展示）。
pub fn snapshot() -> Vec<BandUsage> {
    BANDS
        .iter()
        .map(|band| {
            let used: Vec<u16> = (band.start..=band.end)
                .filter(|port| !is_free(*port))
                .collect();
            let total = u32::from(band.end) - u32::from(band.start) + 1;
            BandUsage {
                key: band.key,
                label: band.label,
                start: band.start,
                end: band.end,
                free: total.saturating_sub(used.len() as u32) as usize,
                used,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_are_well_formed() {
        for band in BANDS.iter() {
            assert!(band.start > FORBIDDEN_BELOW, "{} 段非法", band.key);
            assert!(band.end >= band.start, "{} 段区间非法", band.key);
        }
        for (i, left) in BANDS.iter().enumerate() {
            for right in BANDS.iter().skip(i + 1) {
                assert!(
                    left.end < right.start || right.end < left.start,
                    "段 {} 与 {} 重叠",
                    left.key,
                    right.key
                );
            }
        }
    }

    #[test]
    fn band_for_app_maps_by_keyword() {
        assert_eq!(band_for_app("rembg-ui", None).key, "media");
        assert_eq!(band_for_app("lyblive", None).key, "livestream");
        assert_eq!(band_for_app("lilyco-manager", None).key, "internal");
        assert_eq!(band_for_app("bank-wallet", None).key, "finance");
        assert_eq!(band_for_app("steam-link", None).key, "game");
        assert_eq!(band_for_app("agent-hub", None).key, "ai");
        assert_eq!(band_for_app("random-cli", None).key, "devtools");
        // 显式 hint 优先，未知 hint 回落到 id 推断
        assert_eq!(band_for_app("random-cli", Some("media")).key, "media");
        assert_eq!(band_for_app("lyblive", Some("nope")).key, "livestream");
    }

    #[test]
    fn allocate_moves_within_band_and_marks_drift() {
        let band = band_by_key("devtools").unwrap();
        // 用段尾附近的端口，避免和真实占用撞车
        let preferred = band.end;
        let got = allocate(band, Some(preferred)).expect("段内应能分配");
        assert!(got.port >= band.start && got.port <= band.end);
        assert_eq!(got.port, preferred, "首选端口空闲时应原样返回");
        assert!(!got.drifted);
    }

    #[test]
    fn allocate_never_crosses_band() {
        let band = band_by_key("internal").unwrap();
        let got = allocate(band, None).expect("内部段应能分配");
        assert!(got.port >= band.start && got.port <= band.end);
    }

    #[test]
    fn allocate_respects_preferred_outside_band() {
        let band = band_by_key("media").unwrap();
        let got = allocate(band, Some(12345)).expect("段外首选应回落到段起点");
        assert_eq!(got.port, band.start);
        assert!(got.drifted);
    }

    #[test]
    fn snapshot_reports_every_band() {
        let rows = snapshot();
        assert_eq!(rows.len(), BANDS.len());
        for row in rows {
            assert!(row.free <= (row.end - row.start + 1) as usize);
        }
    }
}
