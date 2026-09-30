//! 桌面端的命令层：不自己解析办公文件，只把参数交给随包带出来的 `lbin`
//!
//! 这份分工是有意的：`lbin` 那批账在 CI 上与第二读者逐格对过（语料 301 份件，每次 run 整本重跑），
//! 桌面端再造一个读者就是多一处会漂移的说法。所以这一层只做三件事 —— 白名单、取输出、
//! 把 JSON 原样交回前端。

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::error::{AppError, Result};

/// 与 `lbin --help` 那本清单同源：名字在这一层写死，前端不拼字符串
const COMMANDS: [(&str, &str); 10] = [
    ("office-info", "这是什么、谁写的、有没有宏"),
    ("office-text", "文件里写了什么（按段落）"),
    ("office-meta", "文档属性那一份账"),
    ("office-doc", "段落 / 标题 / 表格 / 链接 / 批注"),
    ("office-sheet", "表清单（含隐藏的）与格子"),
    ("office-slide", "放映顺序、每页标题与备注"),
    ("office-package", "包自证：关系与内容类型"),
    ("office-objects", "嵌入物、外链、宏与加密"),
    ("office-pdf", "页、字体、表单、批注、权限"),
    ("identify", "不像办公件时：这是什么容器"),
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub name: String,
    pub hint: String,
}

/// 前端渲染那排按钮用的一本协议，不各写各的
#[tauri::command]
pub fn office_commands() -> Vec<CommandInfo> {
    COMMANDS
        .iter()
        .map(|(name, hint)| CommandInfo {
            name: (*name).to_string(),
            hint: (*hint).to_string(),
        })
        .collect()
}

/// 跑一次：`lbin <command> --path <file> --json`
///
/// `--json` 恒加：这一层给前端的必须是结构，不是给人看的排版；页面自己按账本渲染
#[tauri::command]
pub async fn office_run(app: AppHandle, command: String, path: String) -> Result<Value> {
    if !COMMANDS.iter().any(|one| one.0 == command) {
        return Err(AppError::Args(format!("不认识的命令：{command}")));
    }
    let file = std::path::Path::new(&path);
    if !file.is_file() {
        return Err(AppError::Args(format!(
            "这不是一个能读的文件：{path}（没跟着文件过来的路径，桌面端读不到）"
        )));
    }
    let target = file.to_string_lossy().to_string();
    // 这里刻意不写出返回值的类型名：那个结构在 tauri-plugin-shell 2.x 里改过名
    // （2.4.0 叫 `process::Output`，更早叫 `process::CommandOutput`），而 CI 上的
    // 版本是按 Cargo.lock 走的。只用它的三个字段，名字就不参与编译，改不动我们。
    let output = app
        .shell()
        .sidecar("lbin")
        .map_err(|one| AppError::Sidecar(one.to_string()))?
        .args([command.as_str(), "--path", target.as_str(), "--json"])
        .output()
        .await
        .map_err(|one| AppError::Spawn(one.to_string()))?;
    if !output.status.success() {
        return Err(AppError::Status(format!(
            "退出码 {:?}：{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|one| AppError::Parse(one.to_string()))
}

#[cfg(test)]
mod tests {
    use super::COMMANDS;

    /// 这一层唯一的自由就是那十个名字：它们必须与 `lbin` 真正认得的子命令一字不差
    #[test]
    fn every_advertised_command_is_a_real_lbin_subcommand() {
        let text = include_str!("../../lilyco-binfmt/src/main.rs");
        for (name, _hint) in COMMANDS {
            let needle = format!("\"{name}\"");
            assert!(
                text.contains(&needle),
                "{name} 在这一层挂着，但 lbin 的源码里没有这个名字：{needle}"
            );
        }
    }

    #[test]
    fn hints_are_not_empty_and_names_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, hint) in COMMANDS {
            assert!(!hint.trim().is_empty(), "{name} 没说人话");
            assert!(seen.insert(name), "{name} 挂了两次");
        }
    }
}
