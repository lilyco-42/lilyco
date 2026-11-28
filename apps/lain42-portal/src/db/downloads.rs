//! 开源软件下载库目录查询、夸克链接维护与下载事件记录数据层。

use super::{as_i64, as_str, DbError, DownloadItem, PortalDb};

impl PortalDb {
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

    pub async fn update_download_quark_url(
        &self,
        slug: &str,
        version: &str,
        quark_url: &str,
    ) -> Result<u64, DbError> {
        let rows = self
            .conn
            .execute(
                "UPDATE downloads_catalog SET quark_url = ?1, version = CASE WHEN ?2 != '' THEN ?2 ELSE version END WHERE slug = ?3",
                (quark_url, version, slug),
            )
            .await?;
        Ok(rows)
    }

    pub async fn record_download_event(
        &self,
        slug: &str,
        channel: &str,
        mirror: &str,
        os: &str,
        arch: &str,
        user_agent: &str,
    ) -> Result<(), DbError> {
        let now = lilyco_license::current_unix_secs() as i64;
        let ua_short: String = user_agent.chars().take(160).collect();
        self.conn
            .execute(
                "INSERT INTO download_events (slug, channel, mirror, os, arch, user_agent, created_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (slug, channel, mirror, os, arch, ua_short.as_str(), now),
            )
            .await?;
        Ok(())
    }
}
