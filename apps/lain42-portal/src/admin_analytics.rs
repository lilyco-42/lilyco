//! Read models for one portal ledger. Cross-site catalog data is kept in a separate response.
use std::sync::LazyLock;
use std::time::Instant;

use serde::Serialize;
use turso::Connection;

use super::{as_i64, as_str, DbError, PortalDb};

pub(crate) static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);
const DAY_SECONDS: i64 = 86_400;

#[derive(Clone, Copy, Serialize)]
pub(crate) struct Period {
    pub days: i64,
    pub start_unix: i64,
    pub end_unix: i64,
}

impl Period {
    pub(crate) fn ending_today(days: i64, now: i64) -> Self {
        Self {
            days,
            start_unix: now.div_euclid(DAY_SECONDS) * DAY_SECONDS - (days - 1) * DAY_SECONDS,
            end_unix: now + 1,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Users {
    total: i64,
    registered: i64,
    active: i64,
}

#[derive(Serialize)]
pub(crate) struct Points {
    paid_balance: i64,
    bonus_balance: i64,
    order_points: i64,
}

#[derive(Serialize)]
pub(crate) struct Commerce {
    pub users: Users,
    pub points: Points,
    pub orders: i64,
    pub products: Vec<ProductOrders>,
}

#[derive(Serialize)]
pub(crate) struct ProductOrders {
    pub code: String,
    pub orders: i64,
    pub points: i64,
}

#[derive(Serialize)]
pub(crate) struct Day {
    day_unix: i64,
    views: i64,
    registrations: i64,
    paid_orders: i64,
}

#[derive(Serialize)]
pub(crate) struct Breakdown {
    key: String,
    count: i64,
}

#[derive(Serialize)]
pub(crate) struct Activity {
    pub timeline: Vec<Day>,
    pub pages: Vec<Breakdown>,
    pub systems: Vec<Breakdown>,
    pub regions: Vec<Breakdown>,
}

pub(crate) struct Visit<'a> {
    pub page: &'a str,
    pub username: Option<&'a str>,
    pub os: &'a str,
    pub country: Option<&'a str>,
}

impl PortalDb {
    pub(super) async fn init_admin_analytics(&self) -> Result<(), DbError> {
        for sql in [
            "CREATE TABLE IF NOT EXISTS portal_visits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                page TEXT NOT NULL,
                user_id INTEGER,
                os TEXT NOT NULL,
                country TEXT,
                created_unix INTEGER NOT NULL
            )",
            "CREATE INDEX IF NOT EXISTS portal_visits_by_time ON portal_visits(created_unix)",
            "CREATE INDEX IF NOT EXISTS studio_users_by_time ON studio_users(created_unix)",
            "CREATE INDEX IF NOT EXISTS studio_orders_by_time ON studio_orders(created_unix)",
        ] {
            self.conn.execute(sql, ()).await?;
        }
        Ok(())
    }

    pub(crate) async fn record_portal_visit(&self, visit: Visit<'_>) -> Result<(), DbError> {
        let unix = lilyco_license::current_unix_secs() as i64;
        self.conn
            .execute(
                "INSERT INTO portal_visits(page,user_id,os,country,created_unix)
             VALUES (?1,(SELECT id FROM studio_users WHERE username=?2),?3,?4,?5)",
                (visit.page, visit.username, visit.os, visit.country, unix),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn admin_commerce(&self, period: Period) -> Result<Commerce, DbError> {
        let db = self._db.connect()?;
        let mut rows = db.query(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN created_unix>=?1 AND created_unix<?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(points_balance),0), COALESCE(SUM(bonus_points),0),
                    (SELECT COUNT(DISTINCT user_id) FROM portal_visits WHERE created_unix>=?1 AND created_unix<?2)
             FROM studio_users",
            (period.start_unix, period.end_unix),
        ).await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| DbError::Decode("用户汇总为空".into()))?;
        let users = Users {
            total: as_i64(row.get_value(0)?)?,
            registered: as_i64(row.get_value(1)?)?,
            active: as_i64(row.get_value(4)?)?,
        };
        let mut points = Points {
            paid_balance: as_i64(row.get_value(2)?)?,
            bonus_balance: as_i64(row.get_value(3)?)?,
            order_points: 0,
        };
        drop(rows);
        let mut records = db
            .query(
                "SELECT product, COUNT(*), COALESCE(SUM(points_amount),0)
             FROM studio_orders WHERE status='paid' AND created_unix>=?1 AND created_unix<?2
             GROUP BY product ORDER BY COUNT(*) DESC, product",
                (period.start_unix, period.end_unix),
            )
            .await?;
        let mut products = Vec::new();
        let mut orders = 0_i64;
        while let Some(record) = records.next().await? {
            let product = ProductOrders {
                code: as_str(record.get_value(0)?)?,
                orders: as_i64(record.get_value(1)?)?,
                points: as_i64(record.get_value(2)?)?,
            };
            orders = orders.saturating_add(product.orders);
            points.order_points = points.order_points.saturating_add(product.points);
            products.push(product);
        }
        Ok(Commerce {
            users,
            points,
            orders,
            products,
        })
    }

    pub(crate) async fn admin_activity(&self, period: Period) -> Result<Activity, DbError> {
        let db = self._db.connect()?;
        let mut timeline: Vec<Day> = (0..period.days)
            .map(|offset| Day {
                day_unix: period.start_unix + offset * DAY_SECONDS,
                views: 0,
                registrations: 0,
                paid_orders: 0,
            })
            .collect();
        let mut rows = db.query(
            "SELECT bucket, kind, COUNT(*) FROM (
                 SELECT created_unix/86400 AS bucket, 0 AS kind FROM portal_visits
                   WHERE created_unix>=?1 AND created_unix<?2
                 UNION ALL
                 SELECT created_unix/86400, 1 FROM studio_users WHERE created_unix>=?1 AND created_unix<?2
                 UNION ALL
                 SELECT created_unix/86400, 2 FROM studio_orders
                   WHERE status='paid' AND created_unix>=?1 AND created_unix<?2
             ) GROUP BY bucket, kind",
            (period.start_unix, period.end_unix),
        ).await?;
        while let Some(row) = rows.next().await? {
            let offset = as_i64(row.get_value(0)?)? - period.start_unix / DAY_SECONDS;
            let day = timeline
                .get_mut(offset as usize)
                .ok_or_else(|| DbError::Decode("活动日期超出查询范围".into()))?;
            let count = as_i64(row.get_value(2)?)?;
            match as_i64(row.get_value(1)?)? {
                0 => day.views = count,
                1 => day.registrations = count,
                2 => day.paid_orders = count,
                _ => return Err(DbError::Decode("无法识别活动类型".into())),
            }
        }
        drop(rows);
        let pages = breakdown(
            &db,
            period,
            "SELECT page,COUNT(*) FROM portal_visits WHERE created_unix>=?1 AND created_unix<?2
             GROUP BY page ORDER BY COUNT(*) DESC,page LIMIT 20",
        )
        .await?;
        let systems = breakdown(
            &db,
            period,
            "SELECT os,COUNT(*) FROM portal_visits WHERE created_unix>=?1 AND created_unix<?2
             GROUP BY os ORDER BY COUNT(*) DESC,os",
        )
        .await?;
        let regions = breakdown(
            &db,
            period,
            "SELECT COALESCE(country,'unknown'),COUNT(*) FROM portal_visits
             WHERE created_unix>=?1 AND created_unix<?2 GROUP BY COALESCE(country,'unknown')
             ORDER BY COUNT(*) DESC,COALESCE(country,'unknown')",
        )
        .await?;
        Ok(Activity {
            timeline,
            pages,
            systems,
            regions,
        })
    }

    pub(crate) async fn admin_db_latency(&self) -> Result<f64, DbError> {
        let start = Instant::now();
        let db = self._db.connect()?;
        let mut rows = db.query("SELECT 1", ()).await?;
        rows.next().await?;
        Ok((start.elapsed().as_secs_f64() * 100_000.0).round() / 100.0)
    }
}

async fn breakdown(db: &Connection, period: Period, sql: &str) -> Result<Vec<Breakdown>, DbError> {
    let mut rows = db.query(sql, (period.start_unix, period.end_unix)).await?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await? {
        result.push(Breakdown {
            key: as_str(row.get_value(0)?)?,
            count: as_i64(row.get_value(1)?)?,
        });
    }
    Ok(result)
}
