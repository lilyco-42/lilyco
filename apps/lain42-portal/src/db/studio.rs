//! Studio 商业账号、双钱包积分账本、订单核销、权益与卡密兑换数据层。

use super::{as_i64, as_str, chrono_iso_from_unix, DbError, PortalDb, StudioUser};

impl PortalDb {
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
                created_unix INTEGER NOT NULL,
                expires_at TEXT NOT NULL DEFAULT '',
                meta_json TEXT NOT NULL DEFAULT '{}'
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
        let _ = self
            .conn
            .execute(
                "ALTER TABLE studio_entitlements ADD COLUMN expires_at TEXT NOT NULL DEFAULT ''",
                (),
            )
            .await;
        let _ = self
            .conn
            .execute(
                "ALTER TABLE studio_entitlements ADD COLUMN meta_json TEXT NOT NULL DEFAULT '{}'",
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

    pub(super) async fn studio_find_user(&self, username: &str) -> Result<StudioUser, DbError> {
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
                "SELECT id, product, status, delivery_status, license_key, created_unix, expires_at, meta_json FROM studio_entitlements WHERE username = ?1 ORDER BY id DESC",
                [user.username.as_str()],
            )
            .await?;
        let mut entitlements = Vec::new();
        while let Some(r) = ent_rows.next().await? {
            let expires_at = as_str(r.get_value(6)?).unwrap_or_default();
            let meta_raw = as_str(r.get_value(7)?).unwrap_or_else(|_| "{}".to_string());
            let meta_val: serde_json::Value =
                serde_json::from_str(&meta_raw).unwrap_or_else(|_| serde_json::json!({}));
            entitlements.push(serde_json::json!({
                "id": as_i64(r.get_value(0)?)?,
                "product": as_str(r.get_value(1)?)?,
                "status": as_str(r.get_value(2)?)?,
                "delivery_status": as_str(r.get_value(3)?)?,
                "license_key": as_str(r.get_value(4)?)?,
                "created_at": as_i64(r.get_value(5)?)?,
                "expires_at": expires_at,
                "meta": meta_val
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

    /// 按订阅令牌或授权码查询生效中的代理权益，返回 `(username, product, meta_json)`。
    pub async fn studio_find_active_proxy_by_token(
        &self,
        token: &str,
    ) -> Result<Option<(String, String, String)>, DbError> {
        let pat_end = format!("%/{token}\"%");
        let pat_mid = format!("%/{token}/%");
        let mut rows = self
            .conn
            .query(
                "SELECT username, product, COALESCE(meta_json, '{}') FROM studio_entitlements \
                 WHERE status = 'active' AND product LIKE 'proxy_%' \
                 AND (license_key = ?1 OR meta_json LIKE ?2 OR meta_json LIKE ?3) \
                 ORDER BY id DESC LIMIT 1",
                (token, pat_end.as_str(), pat_mid.as_str()),
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
        let (duration_days, traffic_gb) = match product {
            "proxy_1m_1g" => (30i64, 10i64),
            "proxy_1y_15g" => (365, 120),
            "proxy_6m" => (30, 40),
            "proxy_1y_140g" => (365, 480),
            "proxy_1m" => (30, 100),
            "proxy_15_3m" => (90, 300),
            "proxy_15_1y" => (365, 1200),
            "proxy_1m_220g" => (30, 220),
            "proxy_3m" => (90, 660),
            "proxy_1y" => (365, 2640),
            "proxy_3y" => (1095, 7920),
            "proxy_pro_1m" => (30, 1000),
            "proxy_pro_3m" => (90, 3000),
            "proxy_pro_1y" => (365, 12000),
            "proxy_pro_3y" => (1095, 36000),
            p if p.ends_with("_annual") || p.ends_with("_1y") => (365, 0),
            _ => (30, 0),
        };
        let expires_at = chrono_iso_from_unix(now + duration_days * 86400);

        let mut sub_url = String::new();
        let mut sub_token = String::new();
        if non_replicable {
            let tok = format!("{:x}{:04x}", (now as u64).wrapping_mul(11400714819323198485u64), user.id);
            sub_token = tok.clone();
            sub_url = format!("https://dev.lain42.top/studio/api/sub/{tok}/clash");
        }
        let clash_url = if !sub_token.is_empty() {
            format!("https://dev.lain42.top/studio/api/sub/{sub_token}/clash")
        } else {
            String::new()
        };
        let base64_url = if !sub_token.is_empty() {
            format!("https://dev.lain42.top/studio/api/sub/{sub_token}/base64")
        } else {
            String::new()
        };
        let meta_val = if non_replicable {
            serde_json::json!({
                "purchase_product": product,
                "traffic_gb": traffic_gb,
                "expires_at": expires_at,
                "sub_url": sub_url
            })
        } else {
            serde_json::json!({ "purchase_product": product, "expires_at": expires_at })
        };
        let meta_str = meta_val.to_string();

        self.conn
            .execute(
                "INSERT INTO studio_orders (order_no, username, product, status, points_amount, created_unix) VALUES (?1, ?2, ?3, 'paid', ?4, ?5)",
                (order_no.as_str(), username, product, points_price, now),
            )
            .await?;
        self.conn
            .execute(
                "INSERT INTO studio_entitlements (username, product, status, delivery_status, license_key, created_unix, expires_at, meta_json) VALUES (?1, ?2, 'active', 'delivered', ?3, ?4, ?5, ?6)",
                (username, product, license_key.as_str(), now, expires_at.as_str(), meta_str.as_str()),
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
                "license_key": license_key,
                "expires_at": expires_at,
                "traffic_gb": traffic_gb,
                "sub_url": sub_url,
                "clash_url": clash_url,
                "base64_url": base64_url,
                "meta": meta_val
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
