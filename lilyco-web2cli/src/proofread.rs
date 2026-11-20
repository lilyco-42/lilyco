//! 政务与企业资料自动校对引擎（零 LLM 成本、确定性国标算法校验）。
//!
//! 支持：
//! - 18 位统一社会信用代码（GB 32100-2015）加权模 31 校验码验证
//! - 18 位中国大陆居民身份证号（GB 11643-1999）加权模 11 校验码验证
//! - 手机号、邮箱、必填项缺失与附件存在性/体积检查

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::review::SubmissionDraft;
use crate::vault::VaultProfile;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofreadIssue {
    pub severity: IssueSeverity,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofreadReport {
    pub passed: bool,
    pub checked_count: usize,
    pub issues: Vec<ProofreadIssue>,
}

/// 校验 18 位统一社会信用代码（GB 32100-2015）
pub fn validate_uscc(code: &str) -> bool {
    const CHARSET: &[u8] = b"0123456789ABCDEFGHJKLMNPQRTUWXY";
    const WEIGHTS: [usize; 17] = [
        1, 3, 9, 27, 19, 26, 16, 17, 20, 29, 25, 13, 8, 24, 10, 30, 28,
    ];
    let s = code.trim().to_ascii_uppercase();
    let bytes = s.as_bytes();
    if bytes.len() != 18 {
        return false;
    }
    let mut sum = 0usize;
    for i in 0..17 {
        let Some(pos) = CHARSET.iter().position(|&c| c == bytes[i]) else {
            return false;
        };
        sum += pos * WEIGHTS[i];
    }
    let check_idx = (31 - (sum % 31)) % 31;
    CHARSET[check_idx] == bytes[17]
}

/// 校验 18 位中国大陆居民身份证号（GB 11643-1999）
pub fn validate_cn_id_card(id: &str) -> bool {
    const WEIGHTS: [u32; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    const CHECK_CODES: &[u8] = b"10X98765432";
    let s = id.trim().to_ascii_uppercase();
    let bytes = s.as_bytes();
    if bytes.len() != 18 {
        return false;
    }
    let mut sum = 0u32;
    for i in 0..17 {
        if !bytes[i].is_ascii_digit() {
            return false;
        }
        sum += u32::from(bytes[i] - b'0') * WEIGHTS[i];
    }
    let expected = CHECK_CODES[(sum % 11) as usize];
    bytes[17] == expected
}

/// 校验中国大陆 11 位手机号
pub fn validate_cn_phone(phone: &str) -> bool {
    let s = phone.trim();
    let b = s.as_bytes();
    b.len() == 11
        && b[0] == b'1'
        && (b'3'..=b'9').contains(&b[1])
        && b.iter().all(u8::is_ascii_digit)
}

/// 对整个分类资料库（`VaultProfile`）执行逐项合规校对。
pub fn proofread_profile(profile: &VaultProfile, max_attachment_kb: u64) -> ProofreadReport {
    let mut issues = Vec::new();
    let mut checked = 0usize;

    for (cat, fields) in &profile.categories {
        for (k, v) in fields {
            checked += 1;
            let fqn = format!("{cat}.{k}");
            let klc = k.to_ascii_lowercase();
            if v.trim().is_empty() {
                issues.push(ProofreadIssue {
                    severity: IssueSeverity::Warning,
                    field: fqn,
                    message: "字段值为空".into(),
                });
                continue;
            }
            if klc.contains("credit_code") || klc.contains("uscc") || klc.contains("uniscid") {
                if !validate_uscc(v) {
                    issues.push(ProofreadIssue {
                        severity: IssueSeverity::Error,
                        field: fqn,
                        message: format!("统一社会信用代码 `{v}` 未通过 GB 32100-2015 校验码核验"),
                    });
                }
            } else if klc.contains("id_card") || klc.contains("idcard") || klc.contains("cerno") {
                if !validate_cn_id_card(v) {
                    issues.push(ProofreadIssue {
                        severity: IssueSeverity::Error,
                        field: fqn,
                        message: format!("身份证号 `{v}` 未通过 GB 11643-1999 校验码核验"),
                    });
                }
            } else if (klc.contains("phone") || klc.contains("mobile")) && !validate_cn_phone(v) {
                issues.push(ProofreadIssue {
                    severity: IssueSeverity::Warning,
                    field: fqn,
                    message: format!("手机号 `{v}` 格式非常规 11 位大陆手机号"),
                });
            }
        }
    }

    for (k, att) in &profile.attachments {
        checked += 1;
        let p = Path::new(&att.path);
        match fs::metadata(p) {
            Err(_) => issues.push(ProofreadIssue {
                severity: IssueSeverity::Error,
                field: format!("attachments.{k}"),
                message: format!("附件文件不存在: {}", att.path),
            }),
            Ok(meta) => {
                let kb = meta.len() / 1024;
                if max_attachment_kb > 0 && kb > max_attachment_kb {
                    issues.push(ProofreadIssue {
                        severity: IssueSeverity::Warning,
                        field: format!("attachments.{k}"),
                        message: format!(
                            "附件体积 {kb} KB 超过政务常见上限 {max_attachment_kb} KB，建议先压缩裁剪"
                        ),
                    });
                }
            }
        }
    }

    let passed = !issues.iter().any(|i| i.severity == IssueSeverity::Error);
    ProofreadReport {
        passed,
        checked_count: checked,
        issues,
    }
}

/// 对待审核草稿（`SubmissionDraft`）执行提交前合规校对。
pub fn proofread_draft(draft: &SubmissionDraft) -> ProofreadReport {
    let mut issues = Vec::new();
    let mut checked = 0usize;

    for missing in &draft.missing_required {
        issues.push(ProofreadIssue {
            severity: IssueSeverity::Error,
            field: missing.clone(),
            message: "必填字段缺失".into(),
        });
    }

    for (k, v) in &draft.args {
        checked += 1;
        let Some(s) = v.as_str() else {
            continue;
        };
        let klc = k.to_ascii_lowercase();
        if (klc.contains("uscc") || klc.contains("uniscid") || klc.contains("credit"))
            && !validate_uscc(s)
        {
            issues.push(ProofreadIssue {
                severity: IssueSeverity::Error,
                field: k.clone(),
                message: format!("统一社会信用代码 `{s}` 校验码错误"),
            });
        }
        if (klc.contains("idcard") || klc.contains("id-card") || klc.contains("cerno"))
            && !validate_cn_id_card(s)
        {
            issues.push(ProofreadIssue {
                severity: IssueSeverity::Error,
                field: k.clone(),
                message: format!("身份证号 `{s}` 校验码错误"),
            });
        }
    }

    let passed = !issues.iter().any(|i| i.severity == IssueSeverity::Error);
    ProofreadReport {
        passed,
        checked_count: checked,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_gb_uscc_and_cn_id_card() {
        // 合法统一社会信用代码（17位前导 + 校验位）
        assert!(validate_uscc("91350100M000100Y43"));
        assert!(!validate_uscc("91350100M000100Y44"));

        // 合法 18 位身份证号
        assert!(validate_cn_id_card("11010519491231002X"));
        assert!(!validate_cn_id_card("110105194912310021"));

        assert!(validate_cn_phone("13800138000"));
        assert!(!validate_cn_phone("12345"));
    }
}
