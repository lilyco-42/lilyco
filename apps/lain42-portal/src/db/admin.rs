//! 运营看板聚合分析、管理员用户/积分/订阅操作与工单数据层。

use turso::Value;

use super::{as_i64, as_str, chrono_iso_from_unix, DbError, PortalDb, StudioUser};

impl PortalDb {
    pub async fn analytics_summary(&self) -> Result<serde_json::Value, DbError> {
        let mut total_events = 0i64;
        let mut quark_clicks = 0i64;
        let mut mirror_clicks = 0i64;
        let mut qr_opens = 0i64;
        let mut page_views = 0i64;

        let mut rows = self
            .conn
            .query(
                "SELECT channel, COUNT(*) FROM download_events GROUP BY channel",
                (),
            )
            .await?;
        let mut channels = Vec::new();
        while let Some(r) = rows.next().await? {
            let ch = as_str(r.get_value(0)?)?;
            let cnt = as_i64(r.get_value(1)?)?;
            total_events += cnt;
            match ch.as_str() {
                "quark" => quark_clicks += cnt,
                "mirror" => mirror_clicks += cnt,
                "quark_qr" => qr_opens += cnt,
                "page_view" => page_views += cnt,
                _ => {}
            }
            channels.push(serde_json::json!({ "channel": ch, "count": cnt }));
        }

        let dl_total = quark_clicks + mirror_clicks;
        let quark_ctr_pct = if dl_total > 0 {
            ((quark_clicks as f64 / dl_total as f64) * 1000.0).round() / 10.0
        } else {
            0.0
        };

        let mut os_rows = self
            .conn
            .query(
                "SELECT CASE WHEN os = '' THEN 'unknown' ELSE os END AS o, COUNT(*) FROM download_events GROUP BY o ORDER BY COUNT(*) DESC LIMIT 8",
                (),
            )
            .await?;
        let mut os_breakdown = Vec::new();
        while let Some(r) = os_rows.next().await? {
            os_breakdown.push(serde_json::json!({
                "os": as_str(r.get_value(0)?)?,
                "count": as_i64(r.get_value(1)?)?
            }));
        }

        let mut top_rows = self
            .conn
            .query(
                "SELECT slug,
                        SUM(CASE WHEN channel = 'quark' THEN 1 ELSE 0 END) AS q_cnt,
                        SUM(CASE WHEN channel = 'mirror' THEN 1 ELSE 0 END) AS m_cnt,
                        COUNT(*) AS tot
                 FROM download_events
                 WHERE channel IN ('quark', 'mirror', 'quark_qr')
                 GROUP BY slug
                 ORDER BY tot DESC
                 LIMIT 12",
                (),
            )
            .await?;
        let mut top_downloads = Vec::new();
        while let Some(r) = top_rows.next().await? {
            top_downloads.push(serde_json::json!({
                "slug": as_str(r.get_value(0)?)?,
                "quark": as_i64(r.get_value(1)?)?,
                "mirror": as_i64(r.get_value(2)?)?,
                "total": as_i64(r.get_value(3)?)?
            }));
        }

        let mut rec_rows = self
            .conn
            .query(
                "SELECT id, slug, channel, mirror, os, arch, created_unix FROM download_events ORDER BY id DESC LIMIT 30",
                (),
            )
            .await?;
        let mut recent_events = Vec::new();
        while let Some(r) = rec_rows.next().await? {
            recent_events.push(serde_json::json!({
                "id": as_i64(r.get_value(0)?)?,
                "slug": as_str(r.get_value(1)?)?,
                "channel": as_str(r.get_value(2)?)?,
                "mirror": as_str(r.get_value(3)?)?,
                "os": as_str(r.get_value(4)?)?,
                "arch": as_str(r.get_value(5)?)?,
                "created_unix": as_i64(r.get_value(6)?)?
            }));
        }

        let total_catalog = self.count_table("downloads_catalog").await.unwrap_or(0);
        let mut q_cat_rows = self
            .conn
            .query("SELECT COUNT(*) FROM downloads_catalog WHERE quark_url != ''", ())
            .await?;
        let quark_ready_catalog = if let Some(r) = q_cat_rows.next().await? {
            as_i64(r.get_value(0)?).unwrap_or(0)
        } else {
            0
        };

        Ok(serde_json::json!({
            "ok": true,
            "totals": {
                "total_events": total_events,
                "quark_clicks": quark_clicks,
                "mirror_clicks": mirror_clicks,
                "qr_opens": qr_opens,
                "page_views": page_views,
                "quark_ctr_pct": quark_ctr_pct,
                "total_catalog": total_catalog,
                "quark_ready_catalog": quark_ready_catalog
            },
            "channel_breakdown": channels,
            "os_breakdown": os_breakdown,
            "top_downloads": top_downloads,
            "recent_events": recent_events
        }))
    }

    pub async fn admin_list_users(&self, query: &str) -> Result<Vec<serde_json::Value>, DbError> {
        let q = query.trim().to_ascii_lowercase();
        let like = format!("%{q}%");
        let mut rows = if q.is_empty() {
            self.conn
                .query(
                    "SELECT id, username, email, points_balance, bonus_points, created_unix
                     FROM studio_users ORDER BY id DESC LIMIT 80",
                    (),
                )
                .await?
        } else {
            self.conn
                .query(
                    "SELECT id, username, email, points_balance, bonus_points, created_unix
                     FROM studio_users
                     WHERE lower(username) LIKE ?1 OR lower(email) LIKE ?1
                     ORDER BY id DESC LIMIT 80",
                    [like.as_str()],
                )
                .await?
        };
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            let uname = as_str(r.get_value(1)?)?;
            let mut ec = self
                .conn
                .query(
                    "SELECT COUNT(*) FROM studio_entitlements WHERE username = ?1 AND status = 'active'",
                    [uname.as_str()],
                )
                .await?;
            let active_cnt = if let Some(er) = ec.next().await? {
                as_i64(er.get_value(0)?).unwrap_or(0)
            } else {
                0
            };
            out.push(serde_json::json!({
                "id": as_i64(r.get_value(0)?)?,
                "username": uname,
                "email": as_str(r.get_value(2)?)?,
                "paid_balance": as_i64(r.get_value(3)?)?,
                "bonus_balance": as_i64(r.get_value(4)?)?,
                "created_unix": as_i64(r.get_value(5)?)?,
                "active_entitlements": active_cnt
            }));
        }
        Ok(out)
    }

    pub async fn admin_update_email(&self, username: &str, email: &str) -> Result<u64, DbError> {
        let uname = username.trim().to_ascii_lowercase();
        let rows = self
            .conn
            .execute(
                "UPDATE studio_users SET email = ?1 WHERE username = ?2",
                (email.trim(), uname.as_str()),
            )
            .await?;
        Ok(rows)
    }

    pub async fn admin_reset_password(
        &self,
        username: &str,
        new_pw_hash: &str,
    ) -> Result<u64, DbError> {
        let uname = username.trim().to_ascii_lowercase();
        let rows = self
            .conn
            .execute(
                "UPDATE studio_users SET password_hash = ?1 WHERE username = ?2",
                (new_pw_hash, uname.as_str()),
            )
            .await?;
        Ok(rows)
    }

    pub async fn admin_adjust_points(
        &self,
        username: &str,
        paid_delta: i64,
        bonus_delta: i64,
    ) -> Result<StudioUser, DbError> {
        let uname = username.trim().to_ascii_lowercase();
        let user = self.studio_find_user(&uname).await?;
        let next_paid = (user.points_balance + paid_delta).max(0);
        let next_bonus = (user.bonus_points + bonus_delta).max(0);
        self.conn
            .execute(
                "UPDATE studio_users SET points_balance = ?1, bonus_points = ?2 WHERE username = ?3",
                (next_paid, next_bonus, uname.as_str()),
            )
            .await?;
        self.studio_find_user(&uname).await
    }

    pub async fn admin_grant_subscription(
        &self,
        username: &str,
        product: &str,
        duration_days: i64,
        traffic_gb: i64,
        sub_url: &str,
    ) -> Result<serde_json::Value, DbError> {
        let uname = username.trim().to_ascii_lowercase();
        let _ = self.studio_find_user(&uname).await?;
        let now = lilyco_license::current_unix_secs() as i64;
        let exp_unix = now + duration_days.max(1) * 86400;
        let expires_at = chrono_iso_from_unix(exp_unix);
        let prefix = if product.starts_with("proxy_pro") {
            "BPRO"
        } else if product.starts_with("proxy_") {
            "PRXY"
        } else {
            "LAIN"
        };
        let key_hex = format!("{:012X}", (now as u64).wrapping_mul(2654435761) & 0xFFFF_FFFF_FFFF);
        let license_key = format!(
            "{prefix}-{}-{}-{}",
            &key_hex[0..4],
            &key_hex[4..8],
            &key_hex[8..12]
        );
        let meta = serde_json::json!({
            "purchase_product": product,
            "traffic_gb": traffic_gb,
            "expires_at": expires_at,
            "sub_url": sub_url.trim(),
            "granted_by": "admin"
        });
        let meta_str = meta.to_string();
        self.conn
            .execute(
                "INSERT INTO studio_entitlements (username, product, status, delivery_status, license_key, created_unix, expires_at, meta_json)
                 VALUES (?1, ?2, 'active', 'delivered', ?3, ?4, ?5, ?6)",
                (
                    uname.as_str(),
                    product,
                    license_key.as_str(),
                    now,
                    expires_at.as_str(),
                    meta_str.as_str(),
                ),
            )
            .await?;
        Ok(serde_json::json!({
            "ok": true,
            "username": uname,
            "product": product,
            "license_key": license_key,
            "expires_at": expires_at,
            "traffic_gb": traffic_gb
        }))
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
