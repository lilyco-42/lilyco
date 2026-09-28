use serde::Serialize;

/// 命令层的失败：一律带上下文，前端拿到就是一句能给人看的话
///
/// 不把它折成「读取失败」四个字的通用信息 —— lbin 那边退出码与 stderr 是这份账有没有读出来的
/// 事实，桌面端也一样
#[derive(Debug)]
pub enum AppError {
    /// sidecar 路径没配好（包内没有 binaries/lbin）
    Sidecar(String),
    /// 起来了但进程没跑成
    Spawn(String),
    /// 退出码非零，连同 stderr 一起交
    Status(String),
    /// stdout 不是 JSON（多半是命令写错或输出被截）
    Parse(String),
    /// 参数就不对：命令不在白名单，或那个路径不是文件
    Args(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sidecar(one) => write!(f, "起不动随包带出来的 lbin：{one}"),
            Self::Spawn(one) => write!(f, "跑 lbin 失败：{one}"),
            Self::Status(one) => write!(f, "lbin 报错了：{one}"),
            Self::Parse(one) => write!(f, "lbin 交回来的不是 JSON：{one}"),
            Self::Args(one) => write!(f, "{one}"),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
