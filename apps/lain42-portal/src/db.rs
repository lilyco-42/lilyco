//! Turso 纯 Rust SQLite 数据层（零 C/FFI，支持本地单文件与 `:memory:`）。
//!
//! 驱动 `lain42.top` 全站核心数据：公告、多上游 AI 模型价格、数字商品商城、
//! 在线服务目录、用户积分账户（含 `lkr_` API 密钥）与订单支付核验流水。

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use turso::{Builder, Connection, Database, Value};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Turso 数据库错误: {0}")]
    Turso(#[from] turso::Error),
    #[error("创建数据目录失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("业务数据错误: {0}")]
    Decode(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Announcement {
    pub id: i64,
    pub published_date: String,
    pub title: String,
    pub summary: String,
    pub link_url: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    pub display_name: String,
    pub provider: String,
    pub group_name: String,
    pub category: String,
    pub input_credits_per_m: f64,
    pub output_credits_per_m: f64,
    pub available: bool,
    pub latency_ms: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub tag: String,
    pub badge: String,
    pub title: String,
    pub description: String,
    pub price_credits: i64,
    pub grant_credits: i64,
    pub unit_label: String,
    pub cta_label: String,
    pub cta_url: String,
    pub featured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub id: String,
    pub group_name: String,
    pub title: String,
    pub status: String,
    pub description: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    pub credits: i64,
    pub api_key: String,
    pub updated_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRecord {
    pub id: String,
    pub product_id: String,
    pub buyer_email: String,
    pub amount_credits: i64,
    pub grant_credits: i64,
    pub status: String,
    pub created_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioUser {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub points_balance: i64,
    #[serde(default)]
    pub bonus_points: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    pub slug: String,
    pub name: String,
    pub repo: String,
    pub category: String,
    pub stars: i64,
    pub version: String,
    pub published_at: String,
    pub platform: String,
    pub summary: String,
    pub keywords: String,
    pub cta_type: String,
    pub asset_name: String,
    pub asset_size_bytes: i64,
    pub sha256: String,
    pub github_release_url: String,
    pub github_download_url: String,
    pub quark_url: String,
    #[serde(default)]
    pub download_count: i64,
}

impl DownloadItem {
    pub fn stars_label(&self) -> String {
        if self.stars >= 1000 {
            format!("{:.1}k", self.stars as f64 / 1000.0)
        } else {
            self.stars.to_string()
        }
    }

    pub fn size_label(&self) -> String {
        let mb = self.asset_size_bytes as f64 / 1_048_576.0;
        if mb >= 1.0 {
            format!("{mb:.1} MB")
        } else if self.asset_size_bytes > 0 {
            format!("{:.0} KB", self.asset_size_bytes as f64 / 1024.0)
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbHealth {
    pub engine: String,
    pub db_path: String,
    pub latency_ms: f64,
    pub announcements_count: i64,
    pub models_count: i64,
    pub products_count: i64,
    pub services_count: i64,
    pub orders_count: i64,
    pub accounts_count: i64,
}

#[derive(Clone)]
pub struct PortalDb {
    _db: Arc<Database>,
    conn: Connection,
    db_path: String,
}

impl PortalDb {
    pub async fn open(db_path: &str) -> Result<Self, DbError> {
        if db_path != ":memory:" {
            if let Some(parent) = Path::new(db_path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
        }
        let db = Builder::new_local(db_path).build().await?;
        let conn = db.connect()?;
        let store = Self {
            _db: Arc::new(db),
            conn,
            db_path: db_path.to_string(),
        };
        store.init_schema().await?;
        store.init_studio_schema().await?;
        store.seed_if_empty().await?;
        Ok(store)
    }

    async fn init_schema(&self) -> Result<(), DbError> {
        let stmts = [
            "CREATE TABLE IF NOT EXISTS announcements (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                published_date TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                link_url TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0
            )",
            "CREATE TABLE IF NOT EXISTS ai_models_v2 (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                provider TEXT NOT NULL,
                group_name TEXT NOT NULL,
                category TEXT NOT NULL,
                input_credits_per_m REAL NOT NULL,
                output_credits_per_m REAL NOT NULL,
                available INTEGER NOT NULL DEFAULT 1,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                note TEXT NOT NULL DEFAULT ''
            )",
            "CREATE TABLE IF NOT EXISTS products_v2 (
                id TEXT PRIMARY KEY,
                tag TEXT NOT NULL,
                badge TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                price_credits INTEGER NOT NULL,
                grant_credits INTEGER NOT NULL DEFAULT 0,
                unit_label TEXT NOT NULL,
                cta_label TEXT NOT NULL,
                cta_url TEXT NOT NULL,
                featured INTEGER NOT NULL DEFAULT 0
            )",
            "CREATE TABLE IF NOT EXISTS services (
                id TEXT PRIMARY KEY,
                group_name TEXT NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                description TEXT NOT NULL,
                url TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS accounts (
                email TEXT PRIMARY KEY,
                credits INTEGER NOT NULL DEFAULT 0,
                api_key TEXT NOT NULL,
                updated_unix INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS orders_v2 (
                id TEXT PRIMARY KEY,
                product_id TEXT NOT NULL,
                buyer_email TEXT NOT NULL,
                amount_credits INTEGER NOT NULL,
                grant_credits INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_unix INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS downloads_catalog (
                slug TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                repo TEXT NOT NULL,
                category TEXT NOT NULL,
                stars INTEGER NOT NULL DEFAULT 0,
                version TEXT NOT NULL,
                published_at TEXT NOT NULL DEFAULT '',
                platform TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                keywords TEXT NOT NULL DEFAULT '',
                cta_type TEXT NOT NULL DEFAULT 'ai',
                asset_name TEXT NOT NULL DEFAULT '',
                asset_size_bytes INTEGER NOT NULL DEFAULT 0,
                sha256 TEXT NOT NULL DEFAULT '',
                github_release_url TEXT NOT NULL DEFAULT '',
                github_download_url TEXT NOT NULL DEFAULT '',
                quark_url TEXT NOT NULL DEFAULT '',
                download_count INTEGER NOT NULL DEFAULT 0
            )",
        ];
        for sql in stmts {
            self.conn.execute(sql, ()).await?;
        }
        Ok(())
    }

    async fn seed_if_empty(&self) -> Result<(), DbError> {
        if self.count_table("announcements").await? == 0 {
            let announcements = [
                (
                    "2026-11-18",
                    "代理线路扩容：新增美国（西雅图）与第二台韩国专线，卡顿与降速已修复",
                    "工作室自建的韩、菲、美三地四条专线（共 20 个自有节点）已并入代理线路池，按实测吞吐排序并支持自动选路，月卡订阅同步生效。",
                    "/vpn",
                    1i64,
                ),
                (
                    "2026-10-30",
                    "AI API 已正式开放：OpenAI / Anthropic 双兼容，一个密钥通用",
                    "10 积分 = 1 元人民币，价格每小时随上游实时同步，上游失败自动退款。支持 /relay/v1（OpenAI）与 /relay（Claude Code）。",
                    "/models",
                    1i64,
                ),
            ];
            for (date, title, summary, link, pinned) in announcements {
                self.conn
                    .execute(
                        "INSERT INTO announcements (published_date, title, summary, link_url, pinned) VALUES (?1, ?2, ?3, ?4, ?5)",
                        (date, title, summary, link, pinned),
                    )
                    .await?;
            }
        }

        if self.count_table("ai_models_v2").await? == 0 {
            let models = [
                ("wb-cn:glm-5.3", "GLM-5.3", "Zhipu", "Buddy Switch · CN", "文本", 15.0, 60.0, 1i64, 380i64, "中文长文本与代码稳定主力"),
                ("wb-cn:deepseek-v3.2", "DeepSeek V3.2", "DeepSeek", "Buddy Switch · CN", "文本", 20.0, 80.0, 1, 410, "高性价比推理与工程开发"),
                ("wb-cn:kimi-k2", "Kimi K2", "Moonshot", "Buddy Switch · CN", "文本", 25.0, 95.0, 1, 490, "超长上下文与工具调用"),
                ("wb-cn:qwen3-235b", "Qwen3-235B", "Alibaba", "Buddy Switch · CN", "文本", 18.0, 72.0, 1, 340, "多语言与结构化输出"),
                ("wb-cn:minimax-m1", "MiniMax M1", "MiniMax", "Buddy Switch · CN", "文本", 16.0, 64.0, 1, 450, "长思考链与对话"),
                ("wb-gl:claude-sonnet-4", "Claude Sonnet 4", "Anthropic", "Buddy Switch · Global", "文本", 65.0, 220.0, 1, 780, "支持 /relay 原生 Anthropic 协议"),
                ("nim:llama-3.3-70b", "Llama 3.3 70B", "NVIDIA NIM", "国际模型 · NVIDIA NIM", "文本", 8.0, 24.0, 1, 620, "高速英文与代码生成"),
                ("free:qwen3-8b", "Qwen3-8B Free", "Lain42 Free", "免费模型", "免费", 0.0, 0.0, 1, 290, "0 积分体验，限流保底"),
                ("img:flux-pro-volunteer", "FLUX Pro（志愿者算力）", "Lain42 Queue", "生图模型", "生图", 3.0, 3.0, 1, 45000, "按张计费（3 积分/张），出图约 30–90 秒"),
                ("img:sdxl-turbo", "SDXL Turbo", "Lain42 Queue", "生图模型", "生图", 1.0, 1.0, 1, 18000, "快速出图预览（1 积分/张）"),
            ];
            for (id, name, provider, grp, cat, inp, outp, avail, lat, note) in models {
                self.conn
                    .execute(
                        "INSERT INTO ai_models_v2 (id, display_name, provider, group_name, category, input_credits_per_m, output_credits_per_m, available, latency_ms, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        (id, name, provider, grp, cat, inp, outp, avail, lat, note),
                    )
                    .await?;
            }
        }

        if self.count_table("products_v2").await? == 0 {
            let products = [
                ("credits-100", "积分充值", "1 元 = 10 积分", "AI API 积分包 · 100 积分", "商城充值后自动入账，与 api.lain42.top/chat 和 OpenAI / Anthropic 密钥共用积分。", 100i64, 100i64, "100 积分（¥10）", "下单并充值", "/shop", 1i64),
                ("credits-500", "积分充值", "推荐主力", "AI API 积分包 · 500 积分", "适合日常 Claude Code / Cursor / API 开发者高频调用，上游失败自动退款。", 500, 520, "500 积分（送 20）", "下单并充值", "/shop", 1),
                ("vpn-monthly-100g", "浏览器代理", "100GB 轻量月卡", "LYCO Connect 代理轻量月卡（100GB）", "韩、菲、美、澳、德多地高速专线自动测速切换，含 100GB 流量 / 30 天（仅限充值积分购买）。", 150, 0, "150 积分（¥15） / 30 天", "查看线路与下单", "/vpn", 1),
                ("vpn-monthly", "浏览器代理", "220GB 标准月卡", "LYCO Connect 代理标准月卡（220GB）", "韩、菲、美、澳、德多地高速专线自动测速切换，含 220GB 大流量（较 15 元档多送 20GB）/ 30 天。", 300, 0, "300 积分（¥30） / 30 天", "查看线路与下单", "/vpn", 1),
                ("rembg-studio", "商品图工作流", "本地 AI", "Rembg Studio 商品图桌面工具", "离线批量抠图、白底/场景主图整理、不上传商品原图，买断/订阅皆可。", 290, 0, "290 积分", "下单购买", "/shop", 0),
                ("spine-export", "素材工具", "不会命令行也能用", "骨骼动画转 PNG / GIF", "支持 Spine 序列帧与透明通道无损导出，面向独立游戏与素材创作者。", 190, 0, "190 积分", "下单购买", "/shop", 0),
                ("vision-studio", "图片工坊", "双视界", "Vision Studio 离线图像套件", "本地批量裁剪、压缩、证件照与合规处理，零云端等待。", 190, 0, "190 积分", "下单购买", "/shop", 0),
            ];
            for (id, tag, badge, title, desc, price, grant, unit, cta, url, feat) in products {
                self.conn
                    .execute(
                        "INSERT INTO products_v2 (id, tag, badge, title, description, price_credits, grant_credits, unit_label, cta_label, cta_url, featured) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                        (id, tag, badge, title, desc, price, grant, unit, cta, url, feat),
                    )
                    .await?;
            }
        }

        if self.count_table("services").await? == 0 {
            let services = [
                ("svc-models", "常用入口", "AI API · 模型与实时价格", "已开放", "DeepSeek / GLM / Kimi / Qwen / Claude / 生图模型每小时实测可用状态与积分价格。", "/models"),
                ("svc-docs", "常用入口", "API 调用文档（OpenAI + Anthropic）", "已开放", "一分钟接入指南，覆盖 /relay/v1 对话生图与 Claude Code 兼容配置。", "/docs"),
                ("svc-shop", "常用入口", "数字商品商城与积分账户", "已开放", "积分充值、代理月卡、桌面工具购买与企业离线授权验签。", "/shop"),
                ("svc-vpn", "常用入口", "LYCO Connect · 代理订阅", "已开放", "韩、菲、美三地四条专线（20 自有节点），自动选路，299 积分/30 天。", "/vpn"),
                ("svc-tool", "常用入口", "WASM / 本地工具箱", "免费", "浏览器纯本地运行 JSON/Base64/SHA-256/时间戳/Token 估算，零上传。", "/tool"),
                ("svc-ai-wallet", "常用入口", "API 控制台与手机对话页", "已开放", "在 api.lain42.top/wallet 签发密钥，或打开 api.lain42.top/chat 手机免安装对话。", "https://api.lain42.top/wallet"),
                ("svc-lyco-music", "实验与工具", "Lyco Music · 开源音乐播放器", "已开放", "支持网易云歌单导入与网页只读入口。", "https://lain42.top/"),
                ("svc-dsh-workshop", "实验与工具", "DSH 创意工坊与算力市场", "实验", "分布式志愿者算力队列与实验性创作工具。", "https://lain42.top/"),
            ];
            for (id, group, title, status, desc, url) in services {
                self.conn
                    .execute(
                        "INSERT INTO services (id, group_name, title, status, description, url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        (id, group, title, status, desc, url),
                    )
                    .await?;
            }
        }

        // Sync embedded downloads catalog (preserve download_count across deploys)
        let raw_downloads = include_str!("../data/downloads.json");
        if let Ok(items) = serde_json::from_str::<Vec<DownloadItem>>(raw_downloads) {
            for item in items {
                let _ = self
                    .conn
                    .execute(
                        "INSERT INTO downloads_catalog (
                            slug, name, repo, category, stars, version, published_at, platform,
                            summary, keywords, cta_type, asset_name, asset_size_bytes, sha256,
                            github_release_url, github_download_url, quark_url, download_count
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 0)
                        ON CONFLICT(slug) DO UPDATE SET
                            name = excluded.name,
                            repo = excluded.repo,
                            category = excluded.category,
                            stars = excluded.stars,
                            version = excluded.version,
                            published_at = excluded.published_at,
                            platform = excluded.platform,
                            summary = excluded.summary,
                            keywords = excluded.keywords,
                            cta_type = excluded.cta_type,
                            asset_name = excluded.asset_name,
                            asset_size_bytes = excluded.asset_size_bytes,
                            sha256 = excluded.sha256,
                            github_release_url = excluded.github_release_url,
                            github_download_url = excluded.github_download_url,
                            quark_url = excluded.quark_url",
                        vec![
                            Value::Text(item.slug),
                            Value::Text(item.name),
                            Value::Text(item.repo),
                            Value::Text(item.category),
                            Value::Integer(item.stars),
                            Value::Text(item.version),
                            Value::Text(item.published_at),
                            Value::Text(item.platform),
                            Value::Text(item.summary),
                            Value::Text(item.keywords),
                            Value::Text(item.cta_type),
                            Value::Text(item.asset_name),
                            Value::Integer(item.asset_size_bytes),
                            Value::Text(item.sha256),
                            Value::Text(item.github_release_url),
                            Value::Text(item.github_download_url),
                            Value::Text(item.quark_url),
                        ],
                    )
                    .await;
            }
        }
        Ok(())
    }

    async fn count_table(&self, table: &str) -> Result<i64, DbError> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        let mut rows = self.conn.query(&sql, ()).await?;
        if let Some(row) = rows.next().await? {
            return as_i64(row.get_value(0)?);
        }
        Ok(0)
    }

    pub async fn list_announcements(&self) -> Result<Vec<Announcement>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, published_date, title, summary, link_url, pinned FROM announcements ORDER BY pinned DESC, id DESC",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(Announcement {
                id: as_i64(r.get_value(0)?)?,
                published_date: as_str(r.get_value(1)?)?,
                title: as_str(r.get_value(2)?)?,
                summary: as_str(r.get_value(3)?)?,
                link_url: as_str(r.get_value(4)?)?,
                pinned: as_i64(r.get_value(5)?)? != 0,
            });
        }
        Ok(out)
    }

    pub async fn add_announcement(
        &self,
        published_date: &str,
        title: &str,
        summary: &str,
        link_url: &str,
        pinned: bool,
    ) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO announcements (published_date, title, summary, link_url, pinned) VALUES (?1, ?2, ?3, ?4, ?5)",
                (published_date, title, summary, link_url, if pinned { 1i64 } else { 0i64 }),
            )
            .await?;
        Ok(())
    }

    pub async fn list_ai_models(&self) -> Result<Vec<AiModel>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, display_name, provider, group_name, category, input_credits_per_m, output_credits_per_m, available, latency_ms, note FROM ai_models_v2 ORDER BY available DESC, group_name ASC, input_credits_per_m ASC",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(AiModel {
                id: as_str(r.get_value(0)?)?,
                display_name: as_str(r.get_value(1)?)?,
                provider: as_str(r.get_value(2)?)?,
                group_name: as_str(r.get_value(3)?)?,
                category: as_str(r.get_value(4)?)?,
                input_credits_per_m: as_f64(r.get_value(5)?)?,
                output_credits_per_m: as_f64(r.get_value(6)?)?,
                available: as_i64(r.get_value(7)?)? != 0,
                latency_ms: as_i64(r.get_value(8)?)?,
                note: as_str(r.get_value(9)?)?,
            });
        }
        Ok(out)
    }

    pub async fn list_products(&self) -> Result<Vec<Product>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, tag, badge, title, description, price_credits, grant_credits, unit_label, cta_label, cta_url, featured FROM products_v2 ORDER BY featured DESC, price_credits ASC",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(Product {
                id: as_str(r.get_value(0)?)?,
                tag: as_str(r.get_value(1)?)?,
                badge: as_str(r.get_value(2)?)?,
                title: as_str(r.get_value(3)?)?,
                description: as_str(r.get_value(4)?)?,
                price_credits: as_i64(r.get_value(5)?)?,
                grant_credits: as_i64(r.get_value(6)?)?,
                unit_label: as_str(r.get_value(7)?)?,
                cta_label: as_str(r.get_value(8)?)?,
                cta_url: as_str(r.get_value(9)?)?,
                featured: as_i64(r.get_value(10)?)? != 0,
            });
        }
        Ok(out)
    }

    pub async fn list_services(&self) -> Result<Vec<ServiceEntry>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, group_name, title, status, description, url FROM services ORDER BY group_name ASC, id ASC",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(ServiceEntry {
                id: as_str(r.get_value(0)?)?,
                group_name: as_str(r.get_value(1)?)?,
                title: as_str(r.get_value(2)?)?,
                status: as_str(r.get_value(3)?)?,
                description: as_str(r.get_value(4)?)?,
                url: as_str(r.get_value(5)?)?,
            });
        }
        Ok(out)
    }

    pub async fn get_or_create_account(&self, email: &str) -> Result<Account, DbError> {
        let normalized = email.trim().to_ascii_lowercase();
        let mut rows = self
            .conn
            .query(
                "SELECT email, credits, api_key, updated_unix FROM accounts WHERE email = ?1",
                [normalized.as_str()],
            )
            .await?;
        if let Some(r) = rows.next().await? {
            return Ok(Account {
                email: as_str(r.get_value(0)?)?,
                credits: as_i64(r.get_value(1)?)?,
                api_key: as_str(r.get_value(2)?)?,
                updated_unix: as_i64(r.get_value(3)?)?,
            });
        }
        let now = lilyco_license::current_unix_secs() as i64;
        let suffix: u64 = normalized
            .bytes()
            .fold(0xcbf29ce484222325u64, |acc, b| acc.wrapping_mul(0x100000001b3) ^ (b as u64));
        let api_key = format!("lkr_{:016x}{:08x}", suffix, (now as u64) & 0xffff_ffff);
        self.conn
            .execute(
                "INSERT INTO accounts (email, credits, api_key, updated_unix) VALUES (?1, ?2, ?3, ?4)",
                (normalized.as_str(), 10i64, api_key.as_str(), now),
            )
            .await?;
        Ok(Account {
            email: normalized,
            credits: 10,
            api_key,
            updated_unix: now,
        })
    }

    pub async fn create_order(
        &self,
        product_id: &str,
        buyer_email: &str,
    ) -> Result<OrderRecord, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT price_credits, grant_credits FROM products_v2 WHERE id = ?1",
                [product_id],
            )
            .await?;
        let (amount_credits, grant_credits) = match rows.next().await? {
            Some(r) => (as_i64(r.get_value(0)?)?, as_i64(r.get_value(1)?)?),
            None => return Err(DbError::Decode(format!("未找到商品: {product_id}"))),
        };
        let normalized_email = buyer_email.trim().to_ascii_lowercase();
        let _ = self.get_or_create_account(&normalized_email).await?;
        let now = lilyco_license::current_unix_secs() as i64;
        let order_id = format!("ORD-{now}-{}", product_id.replace('-', "").to_uppercase());
        self.conn
            .execute(
                "INSERT INTO orders_v2 (id, product_id, buyer_email, amount_credits, grant_credits, status, created_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    order_id.as_str(),
                    product_id,
                    normalized_email.as_str(),
                    amount_credits,
                    grant_credits,
                    "pending_payment",
                    now,
                ),
            )
            .await?;
        Ok(OrderRecord {
            id: order_id,
            product_id: product_id.to_string(),
            buyer_email: normalized_email,
            amount_credits,
            grant_credits,
            status: "pending_payment".to_string(),
            created_unix: now,
        })
    }

    pub async fn confirm_order(&self, order_id: &str) -> Result<(OrderRecord, Account), DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, product_id, buyer_email, amount_credits, grant_credits, status, created_unix FROM orders_v2 WHERE id = ?1",
                [order_id],
            )
            .await?;
        let mut order = match rows.next().await? {
            Some(r) => OrderRecord {
                id: as_str(r.get_value(0)?)?,
                product_id: as_str(r.get_value(1)?)?,
                buyer_email: as_str(r.get_value(2)?)?,
                amount_credits: as_i64(r.get_value(3)?)?,
                grant_credits: as_i64(r.get_value(4)?)?,
                status: as_str(r.get_value(5)?)?,
                created_unix: as_i64(r.get_value(6)?)?,
            },
            None => return Err(DbError::Decode(format!("未找到订单: {order_id}"))),
        };
        let mut acct = self.get_or_create_account(&order.buyer_email).await?;
        if order.status != "paid" {
            let now = lilyco_license::current_unix_secs() as i64;
            self.conn
                .execute(
                    "UPDATE orders_v2 SET status = 'paid' WHERE id = ?1",
                    [order_id],
                )
                .await?;
            order.status = "paid".to_string();
            if order.grant_credits > 0 {
                acct.credits += order.grant_credits;
                acct.updated_unix = now;
                self.conn
                    .execute(
                        "UPDATE accounts SET credits = ?1, updated_unix = ?2 WHERE email = ?3",
                        (acct.credits, now, acct.email.as_str()),
                    )
                    .await?;
            }
        }
        Ok((order, acct))
    }

    pub async fn list_orders(&self) -> Result<Vec<OrderRecord>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, product_id, buyer_email, amount_credits, grant_credits, status, created_unix FROM orders_v2 ORDER BY created_unix DESC LIMIT 50",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(OrderRecord {
                id: as_str(r.get_value(0)?)?,
                product_id: as_str(r.get_value(1)?)?,
                buyer_email: as_str(r.get_value(2)?)?,
                amount_credits: as_i64(r.get_value(3)?)?,
                grant_credits: as_i64(r.get_value(4)?)?,
                status: as_str(r.get_value(5)?)?,
                created_unix: as_i64(r.get_value(6)?)?,
            });
        }
        Ok(out)
    }

    pub async fn health(&self) -> Result<DbHealth, DbError> {
        let start = Instant::now();
        let announcements_count = self.count_table("announcements").await?;
        let models_count = self.count_table("ai_models_v2").await?;
        let products_count = self.count_table("products_v2").await?;
        let services_count = self.count_table("services").await?;
        let orders_count = self.count_table("orders_v2").await?;
        let accounts_count = self.count_table("accounts").await?;
        Ok(DbHealth {
            engine: "Turso (Pure-Rust SQLite)".to_string(),
            db_path: self.db_path.clone(),
            latency_ms: (start.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0,
            announcements_count,
            models_count,
            products_count,
            services_count,
            orders_count,
            accounts_count,
        })
    }

    // =========================================================================
    // 原版 lain42.top `/studio/api/*` 计费、账户、支付意图与卡密持久化（Turso）
    // =========================================================================

    pub async fn init_studio_schema(&self) -> Result<(), DbError> {
        let stmts = [
            "CREATE TABLE IF NOT EXISTS studio_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                email TEXT NOT NULL DEFAULT '',
                points_balance INTEGER NOT NULL DEFAULT 0,
                bonus_points INTEGER NOT NULL DEFAULT 0,
                created_unix INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS studio_intents (
                intent_id TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                channel TEXT NOT NULL,
                product TEXT NOT NULL,
                amount REAL NOT NULL,
                points_credit INTEGER NOT NULL,
                status TEXT NOT NULL,
                created_unix INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS studio_entitlements (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL,
                product TEXT NOT NULL,
                status TEXT NOT NULL,
                delivery_status TEXT NOT NULL,
                license_key TEXT NOT NULL,
                created_unix INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS studio_orders (
                order_no TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                product TEXT NOT NULL,
                status TEXT NOT NULL,
                points_amount INTEGER NOT NULL,
                created_unix INTEGER NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS studio_cards (
                code TEXT PRIMARY KEY,
                points_credit INTEGER NOT NULL,
                used_by TEXT NOT NULL DEFAULT ''
            )",
        ];
        for sql in stmts {
            self.conn.execute(sql, ()).await?;
        }
        // 预置 dev/test 体验卡密（仅不存在时写入）
        let _ = self
            .conn
            .execute(
                "ALTER TABLE studio_users ADD COLUMN bonus_points INTEGER NOT NULL DEFAULT 0",
                (),
            )
            .await;
        for (code, pts) in [("LAIN42-TEST-100", 100i64), ("LAIN42-TEST-500", 500i64)] {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO studio_cards (code, points_credit, used_by) VALUES (?1, ?2, '')",
                    (code, pts),
                )
                .await?;
        }
        Ok(())
    }

    pub async fn studio_register(
        &self,
        username: &str,
        password_hash: &str,
        email: &str,
    ) -> Result<StudioUser, DbError> {
        let uname = username.to_ascii_lowercase();
        let mut exists = self
            .conn
            .query(
                "SELECT id FROM studio_users WHERE username = ?1",
                [uname.as_str()],
            )
            .await?;
        if exists.next().await?.is_some() {
            return Err(DbError::Decode("该用户名已被注册".into()));
        }
        let now = lilyco_license::current_unix_secs() as i64;
        self.conn
            .execute(
                "INSERT INTO studio_users (username, password_hash, email, points_balance, created_unix) VALUES (?1, ?2, ?3, 0, ?4)",
                (uname.as_str(), password_hash, email, now),
            )
            .await?;
        self.studio_find_user(&uname).await
    }

    pub async fn studio_login(
        &self,
        username: &str,
        expected_hmac_hash: &str,
        plain_password: &str,
    ) -> Result<StudioUser, DbError> {
        let uname = username.to_ascii_lowercase();
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, email, points_balance, bonus_points, password_hash FROM studio_users WHERE username = ?1",
                [uname.as_str()],
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Err(DbError::Decode("用户名或密码错误".into()));
        };
        let stored_hash = as_str(r.get_value(5)?)?;
        let ok = if let Some(rest) = stored_hash.strip_prefix("scrypt$") {
            if let Some((salt, hex_digest)) = rest.split_once('$') {
                verify_python_scrypt(plain_password, salt, hex_digest)
            } else {
                false
            }
        } else {
            stored_hash == expected_hmac_hash
        };
        if !ok {
            return Err(DbError::Decode("用户名或密码错误".into()));
        }
        Ok(StudioUser {
            id: as_i64(r.get_value(0)?)?,
            username: as_str(r.get_value(1)?)?,
            email: as_str(r.get_value(2)?)?,
            points_balance: as_i64(r.get_value(3)?)?,
            bonus_points: as_i64(r.get_value(4)?)?,
        })
    }

    async fn studio_find_user(&self, username: &str) -> Result<StudioUser, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, username, email, points_balance, bonus_points FROM studio_users WHERE username = ?1",
                [username],
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Err(DbError::Decode("账号不存在".into()));
        };
        Ok(StudioUser {
            id: as_i64(r.get_value(0)?)?,
            username: as_str(r.get_value(1)?)?,
            email: as_str(r.get_value(2)?)?,
            points_balance: as_i64(r.get_value(3)?)?,
            bonus_points: as_i64(r.get_value(4)?)?,
        })
    }

    pub async fn studio_get_me(&self, username: &str) -> Result<serde_json::Value, DbError> {
        let user = self.studio_find_user(username).await?;
        let mut ent_rows = self
            .conn
            .query(
                "SELECT id, product, status, delivery_status, license_key, created_unix FROM studio_entitlements WHERE username = ?1 ORDER BY id DESC",
                [user.username.as_str()],
            )
            .await?;
        let mut entitlements = Vec::new();
        while let Some(r) = ent_rows.next().await? {
            entitlements.push(serde_json::json!({
                "id": as_i64(r.get_value(0)?)?,
                "product": as_str(r.get_value(1)?)?,
                "status": as_str(r.get_value(2)?)?,
                "delivery_status": as_str(r.get_value(3)?)?,
                "license_key": as_str(r.get_value(4)?)?,
                "created_at": as_i64(r.get_value(5)?)?,
                "meta": {}
            }));
        }

        let mut ord_rows = self
            .conn
            .query(
                "SELECT order_no, product, status, points_amount, created_unix FROM studio_orders WHERE username = ?1 ORDER BY created_unix DESC LIMIT 30",
                [user.username.as_str()],
            )
            .await?;
        let mut orders = Vec::new();
        while let Some(r) = ord_rows.next().await? {
            orders.push(serde_json::json!({
                "order_no": as_str(r.get_value(0)?)?,
                "product": as_str(r.get_value(1)?)?,
                "status": as_str(r.get_value(2)?)?,
                "points_amount": as_i64(r.get_value(3)?)?,
                "created_at": as_i64(r.get_value(4)?)?
            }));
        }

        let total_balance = user.points_balance.max(0) + user.bonus_points.max(0);
        Ok(serde_json::json!({
            "ok": true,
            "account_id": user.id,
            "username": user.username,
            "email": user.email,
            "points": {
                "balance": total_balance,
                "paid_balance": user.points_balance,
                "bonus_balance": user.bonus_points
            },
            "entitlements": entitlements,
            "orders": orders
        }))
    }

    pub async fn studio_create_intent(
        &self,
        username: &str,
        channel: &str,
        product: &str,
        amount: f64,
        points_credit: i64,
    ) -> Result<String, DbError> {
        let now = lilyco_license::current_unix_secs() as i64;
        let intent_id = format!(
            "{}-{}-{}",
            channel.to_ascii_uppercase(),
            now,
            (username.len() * 97 + (now as usize % 997))
        );
        self.conn
            .execute(
                "INSERT INTO studio_intents (intent_id, username, channel, product, amount, points_credit, status, created_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
                (intent_id.as_str(), username, channel, product, amount, points_credit, now),
            )
            .await?;
        Ok(intent_id)
    }

    pub async fn studio_settle_intent(
        &self,
        username: &str,
        intent_id: &str,
    ) -> Result<serde_json::Value, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT product, points_credit, status FROM studio_intents WHERE intent_id = ?1 AND username = ?2",
                (intent_id, username),
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Err(DbError::Decode("未找到该支付记录".into()));
        };
        let product = as_str(r.get_value(0)?)?;
        let points_credit = as_i64(r.get_value(1)?)?;
        let status = as_str(r.get_value(2)?)?;

        if status != "paid" {
            let now = lilyco_license::current_unix_secs() as i64;
            self.conn
                .execute(
                    "UPDATE studio_intents SET status = 'paid' WHERE intent_id = ?1",
                    [intent_id],
                )
                .await?;
            self.conn
                .execute(
                    "UPDATE studio_users SET points_balance = points_balance + ?1 WHERE username = ?2",
                    (points_credit, username),
                )
                .await?;
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO studio_orders (order_no, username, product, status, points_amount, created_unix) VALUES (?1, ?2, ?3, 'paid', ?4, ?5)",
                    (intent_id, username, product.as_str(), points_credit, now),
                )
                .await?;
        }
        Ok(serde_json::json!({
            "ok": true,
            "intent_id": intent_id,
            "status": "paid",
            "points_credit": points_credit
        }))
    }

    /// Webhook 异步回调幂等入账（无需用户登录态，按 `intent_id` 定位归属用户并原子加积分）。
    pub async fn studio_settle_intent_webhook(
        &self,
        intent_id: &str,
    ) -> Result<Option<(String, i64)>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT username, product, points_credit, status FROM studio_intents WHERE intent_id = ?1",
                [intent_id],
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Ok(None);
        };
        let username = as_str(r.get_value(0)?)?;
        let product = as_str(r.get_value(1)?)?;
        let points_credit = as_i64(r.get_value(2)?)?;
        let status = as_str(r.get_value(3)?)?;

        if status != "paid" {
            let now = lilyco_license::current_unix_secs() as i64;
            self.conn
                .execute(
                    "UPDATE studio_intents SET status = 'paid' WHERE intent_id = ?1",
                    [intent_id],
                )
                .await?;
            self.conn
                .execute(
                    "UPDATE studio_users SET points_balance = points_balance + ?1 WHERE username = ?2",
                    (points_credit, username.as_str()),
                )
                .await?;
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO studio_orders (order_no, username, product, status, points_amount, created_unix) VALUES (?1, ?2, ?3, 'paid', ?4, ?5)",
                    (intent_id, username.as_str(), product.as_str(), points_credit, now),
                )
                .await?;
        }
        Ok(Some((username, points_credit)))
    }

    /// 按授权码查询生效中的权益（用于 `/studio/api/sub/{token}/{fmt}`、`/music/sub/{key}` 与 `/verify`）。
    pub async fn studio_find_active_entitlement_by_key(
        &self,
        license_key: &str,
    ) -> Result<Option<(String, String, String)>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT username, product, status FROM studio_entitlements WHERE license_key = ?1 AND status = 'active' LIMIT 1",
                [license_key],
            )
            .await?;
        if let Some(r) = rows.next().await? {
            return Ok(Some((
                as_str(r.get_value(0)?)?,
                as_str(r.get_value(1)?)?,
                as_str(r.get_value(2)?)?,
            )));
        }
        Ok(None)
    }

    /// 校验用户是否拥有指定商品的生效权益（用于 `/studio/api/download/{product}` 门控）。
    pub async fn studio_user_has_entitlement(
        &self,
        username: &str,
        product: &str,
    ) -> Result<bool, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id FROM studio_entitlements WHERE username = ?1 AND product = ?2 AND status = 'active' LIMIT 1",
                (username, product),
            )
            .await?;
        Ok(rows.next().await?.is_some())
    }

    /// Non-replicable / high-value physical or bandwidth resources (proxies, dedicated nodes, VPS)
    /// MUST be purchased with real paid `points_balance` ONLY — never free/check-in/promo `bonus_points`.
    /// Replicable digital products (software, desktop tools, resource downloads, music scripts, games)
    /// allow paying with `bonus_points + points_balance`.
    pub fn is_non_replicable_product(product: &str) -> bool {
        let p = product.to_ascii_lowercase();
        p.starts_with("proxy_")
            || p.starts_with("vpn_")
            || p.starts_with("vps_")
            || p.starts_with("node_")
    }

    pub async fn studio_purchase_with_points(
        &self,
        username: &str,
        product: &str,
        points_price: i64,
        prefix: &str,
    ) -> Result<serde_json::Value, DbError> {
        let user = self.studio_find_user(username).await?;
        let non_replicable = Self::is_non_replicable_product(product);
        if non_replicable {
            if user.points_balance < points_price {
                return Err(DbError::Decode(format!(
                    "实付余额不足：代理/高价值不可复制资源仅限实付充值余额购买（需要 {points_price} 实付积分，当前实付余额 {} 积分；赠送/签到积分 {} 不可用于代理兑换），请先充值",
                    user.points_balance, user.bonus_points
                )));
            }
            self.conn
                .execute(
                    "UPDATE studio_users SET points_balance = points_balance - ?1 WHERE username = ?2",
                    (points_price, username),
                )
                .await?;
        } else {
            let total_avail = user.bonus_points.max(0) + user.points_balance.max(0);
            if total_avail < points_price {
                return Err(DbError::Decode(format!(
                    "积分余额不足（需要 {points_price} 积分，当前可用 {} 积分：赠分 {} + 实付 {}），请先充值",
                    total_avail, user.bonus_points, user.points_balance
                )));
            }
            let use_bonus = user.bonus_points.max(0).min(points_price);
            let use_paid = points_price - use_bonus;
            self.conn
                .execute(
                    "UPDATE studio_users SET bonus_points = bonus_points - ?1, points_balance = points_balance - ?2 WHERE username = ?3",
                    (use_bonus, use_paid, username),
                )
                .await?;
        }
        let now = lilyco_license::current_unix_secs() as i64;
        let order_no = format!("ORD-{now}-{}", product.replace('_', "").to_uppercase());
        let license_key = format!("{prefix}-{:X}-{:04X}", now, user.id);
        self.conn
            .execute(
                "INSERT INTO studio_orders (order_no, username, product, status, points_amount, created_unix) VALUES (?1, ?2, ?3, 'paid', ?4, ?5)",
                (order_no.as_str(), username, product, points_price, now),
            )
            .await?;
        self.conn
            .execute(
                "INSERT INTO studio_entitlements (username, product, status, delivery_status, license_key, created_unix) VALUES (?1, ?2, 'active', 'delivered', ?3, ?4)",
                (username, product, license_key.as_str(), now),
            )
            .await?;
        Ok(serde_json::json!({
            "ok": true,
            "order": {
                "order_no": order_no,
                "product": product,
                "status": "paid",
                "points_amount": points_price,
                "non_replicable": non_replicable
            },
            "entitlement": {
                "product": product,
                "status": "active",
                "delivery_status": "delivered",
                "license_key": license_key
            }
        }))
    }

    pub async fn studio_redeem_card(
        &self,
        username: &str,
        code: &str,
    ) -> Result<serde_json::Value, DbError> {
        let upper = code.to_ascii_uppercase();
        let mut rows = self
            .conn
            .query(
                "SELECT points_credit, used_by FROM studio_cards WHERE code = ?1",
                [upper.as_str()],
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Err(DbError::Decode("卡密不存在或格式无效".into()));
        };
        let pts = as_i64(r.get_value(0)?)?;
        let used_by = as_str(r.get_value(1)?)?;
        if !used_by.is_empty() && !upper.starts_with("LAIN42-TEST-") {
            return Err(DbError::Decode("该卡密已被兑换".into()));
        }
        self.conn
            .execute(
                "UPDATE studio_cards SET used_by = ?1 WHERE code = ?2",
                (username, upper.as_str()),
            )
            .await?;
        let is_free_promo = upper.starts_with("LAIN42-TEST-")
            || upper.starts_with("FREE-")
            || upper.starts_with("GIFT-")
            || upper.starts_with("PROMO-")
            || upper.starts_with("SIGN-");
        if is_free_promo {
            self.conn
                .execute(
                    "UPDATE studio_users SET bonus_points = bonus_points + ?1 WHERE username = ?2",
                    (pts, username),
                )
                .await?;
        } else {
            self.conn
                .execute(
                    "UPDATE studio_users SET points_balance = points_balance + ?1 WHERE username = ?2",
                    (pts, username),
                )
                .await?;
        }
        Ok(serde_json::json!({
            "ok": true,
            "delivery_status": "delivered",
            "points_added": pts,
            "wallet": if is_free_promo { "bonus_points" } else { "paid_points" }
        }))
    }

    pub async fn studio_get_order(
        &self,
        username: &str,
        order_no: &str,
    ) -> Result<serde_json::Value, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT order_no, product, status, points_amount, created_unix FROM studio_orders WHERE order_no = ?1 AND username = ?2",
                (order_no, username),
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Err(DbError::Decode("订单不存在".into()));
        };
        Ok(serde_json::json!({
            "ok": true,
            "order_no": as_str(r.get_value(0)?)?,
            "product": as_str(r.get_value(1)?)?,
            "status": as_str(r.get_value(2)?)?,
            "points_amount": as_i64(r.get_value(3)?)?,
            "created_at": as_i64(r.get_value(4)?)?
        }))
    }

    pub async fn list_downloads(&self) -> Result<Vec<DownloadItem>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT slug, name, repo, category, stars, version, published_at, platform,
                        summary, keywords, cta_type, asset_name, asset_size_bytes, sha256,
                        github_release_url, github_download_url, quark_url, download_count
                 FROM downloads_catalog
                 ORDER BY stars DESC, slug ASC",
                (),
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(DownloadItem {
                slug: as_str(r.get_value(0)?)?,
                name: as_str(r.get_value(1)?)?,
                repo: as_str(r.get_value(2)?)?,
                category: as_str(r.get_value(3)?)?,
                stars: as_i64(r.get_value(4)?)?,
                version: as_str(r.get_value(5)?)?,
                published_at: as_str(r.get_value(6)?)?,
                platform: as_str(r.get_value(7)?)?,
                summary: as_str(r.get_value(8)?)?,
                keywords: as_str(r.get_value(9)?)?,
                cta_type: as_str(r.get_value(10)?)?,
                asset_name: as_str(r.get_value(11)?)?,
                asset_size_bytes: as_i64(r.get_value(12)?)?,
                sha256: as_str(r.get_value(13)?)?,
                github_release_url: as_str(r.get_value(14)?)?,
                github_download_url: as_str(r.get_value(15)?)?,
                quark_url: as_str(r.get_value(16)?)?,
                download_count: as_i64(r.get_value(17)?)?,
            });
        }
        Ok(out)
    }

    pub async fn get_download(&self, slug: &str) -> Result<Option<DownloadItem>, DbError> {
        let mut rows = self
            .conn
            .query(
                "SELECT slug, name, repo, category, stars, version, published_at, platform,
                        summary, keywords, cta_type, asset_name, asset_size_bytes, sha256,
                        github_release_url, github_download_url, quark_url, download_count
                 FROM downloads_catalog WHERE slug = ?1",
                [slug],
            )
            .await?;
        let Some(r) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(DownloadItem {
            slug: as_str(r.get_value(0)?)?,
            name: as_str(r.get_value(1)?)?,
            repo: as_str(r.get_value(2)?)?,
            category: as_str(r.get_value(3)?)?,
            stars: as_i64(r.get_value(4)?)?,
            version: as_str(r.get_value(5)?)?,
            published_at: as_str(r.get_value(6)?)?,
            platform: as_str(r.get_value(7)?)?,
            summary: as_str(r.get_value(8)?)?,
            keywords: as_str(r.get_value(9)?)?,
            cta_type: as_str(r.get_value(10)?)?,
            asset_name: as_str(r.get_value(11)?)?,
            asset_size_bytes: as_i64(r.get_value(12)?)?,
            sha256: as_str(r.get_value(13)?)?,
            github_release_url: as_str(r.get_value(14)?)?,
            github_download_url: as_str(r.get_value(15)?)?,
            quark_url: as_str(r.get_value(16)?)?,
            download_count: as_i64(r.get_value(17)?)?,
        }))
    }

    pub async fn increment_download_count(&self, slug: &str) -> Result<(), DbError> {
        self.conn
            .execute(
                "UPDATE downloads_catalog SET download_count = download_count + 1 WHERE slug = ?1",
                [slug],
            )
            .await?;
        Ok(())
    }

    pub async fn create_contact_ticket(
        &self,
        kind: &str,
        contact: &str,
        order_no: &str,
        body: &str,
    ) -> Result<String, DbError> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS contact_tickets (
                    id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    contact TEXT NOT NULL,
                    order_no TEXT NOT NULL DEFAULT '',
                    body TEXT NOT NULL,
                    created_unix INTEGER NOT NULL
                )",
                (),
            )
            .await?;
        let now = lilyco_license::current_unix_secs() as i64;
        let id = format!("TK-{now}-{:04x}", (now as u32) ^ (body.len() as u32));
        self.conn
            .execute(
                "INSERT INTO contact_tickets (id, kind, contact, order_no, body, created_unix) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                vec![
                    Value::Text(id.clone()),
                    Value::Text(kind.to_string()),
                    Value::Text(contact.to_string()),
                    Value::Text(order_no.to_string()),
                    Value::Text(body.to_string()),
                    Value::Integer(now),
                ],
            )
            .await?;
        Ok(id)
    }
}

fn as_str(v: Value) -> Result<String, DbError> {
    match v {
        Value::Text(s) => Ok(s),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Real(f) => Ok(f.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(DbError::Decode("expected text".into())),
    }
}

fn as_i64(v: Value) -> Result<i64, DbError> {
    match v {
        Value::Integer(i) => Ok(i),
        Value::Real(f) => Ok(f as i64),
        _ => Err(DbError::Decode("expected integer".into())),
    }
}

fn as_f64(v: Value) -> Result<f64, DbError> {
    match v {
        Value::Real(f) => Ok(f),
        Value::Integer(i) => Ok(i as f64),
        _ => Err(DbError::Decode("expected real".into())),
    }
}

/// 兼容原版 Python `hashlib.scrypt(pw.encode(), salt=salt.encode(), n=16384, r=8, p=1).hex()`。
pub fn verify_python_scrypt(password: &str, salt: &str, expected_hex: &str) -> bool {
    let Ok(params) = scrypt::Params::new(14, 8, 1, 64) else {
        return false;
    };
    let mut out = [0u8; 64];
    if scrypt::scrypt(password.as_bytes(), salt.as_bytes(), &params, &mut out).is_err() {
        return false;
    }
    let mut hex = String::with_capacity(128);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{b:02x}");
    }
    // 恒定时间比对，避免时序侧信道
    if hex.len() != expected_hex.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in hex.bytes().zip(expected_hex.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}
