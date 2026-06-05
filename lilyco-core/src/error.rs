use serde::de::{self, EnumAccess, VariantAccess};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// App 的统一错误类型
///
/// 覆盖参数错误、输入无效、运行错误、取消信号、IO 和序列化等场景。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("参数错误: {0}")]
    InvalidArg(String),

    #[error("输入无效: {0}")]
    InvalidInput(String),

    #[error("执行失败: {0}")]
    Runtime(String),

    #[error("已取消")]
    Cancelled,

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),
}

// ── Serialize ──────────────────────────────────────────────

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AppError::Cancelled => {
                serializer.serialize_unit_variant("AppError", 3, "Cancelled")
            }
            _ => {
                let msg = self.message();
                serializer.serialize_newtype_variant(
                    "AppError",
                    self.variant_index(),
                    self.variant_name(),
                    &msg,
                )
            }
        }
    }
}

// ── Deserialize ────────────────────────────────────────────

impl<'de> Deserialize<'de> for AppError {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_enum("AppError", VARIANTS, AppErrorVisitor)
    }
}

const VARIANTS: &[&str] = &[
    "InvalidArg",
    "InvalidInput",
    "Runtime",
    "Cancelled",
    "Io",
    "Serialize",
];

struct AppErrorVisitor;

impl<'de> de::Visitor<'de> for AppErrorVisitor {
    type Value = AppError;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("an AppError variant")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        let (variant, data): (&str, _) = data.variant()?;
        match variant {
            "InvalidArg" => Ok(AppError::InvalidArg(data.newtype_variant()?)),
            "InvalidInput" => Ok(AppError::InvalidInput(data.newtype_variant()?)),
            "Runtime" => Ok(AppError::Runtime(data.newtype_variant()?)),
            "Cancelled" => {
                data.unit_variant()?;
                Ok(AppError::Cancelled)
            }
            "Io" => {
                let msg: String = data.newtype_variant()?;
                Ok(AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    msg,
                )))
            }
            "Serialize" => {
                let msg: String = data.newtype_variant()?;
                Ok(AppError::Serialize(serde_json::Error::io(
                    std::io::Error::new(std::io::ErrorKind::Other, msg),
                )))
            }
            _ => Err(de::Error::unknown_variant(variant, VARIANTS)),
        }
    }
}

// ── helpers ────────────────────────────────────────────────

impl AppError {
    fn variant_index(&self) -> u32 {
        match self {
            AppError::InvalidArg(_) => 0,
            AppError::InvalidInput(_) => 1,
            AppError::Runtime(_) => 2,
            AppError::Cancelled => 3,
            AppError::Io(_) => 4,
            AppError::Serialize(_) => 5,
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            AppError::InvalidArg(_) => "InvalidArg",
            AppError::InvalidInput(_) => "InvalidInput",
            AppError::Runtime(_) => "Runtime",
            AppError::Cancelled => "Cancelled",
            AppError::Io(_) => "Io",
            AppError::Serialize(_) => "Serialize",
        }
    }

    fn message(&self) -> String {
        match self {
            AppError::InvalidArg(msg)
            | AppError::InvalidInput(msg)
            | AppError::Runtime(msg) => msg.clone(),
            AppError::Cancelled => String::new(),
            AppError::Io(e) => e.to_string(),
            AppError::Serialize(e) => e.to_string(),
        }
    }
}
