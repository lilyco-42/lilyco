//! lpxls 域内共享工具：目标文件收集 + 极简 glob（* / ?，不区分大小写）。

use std::path::{Path, PathBuf};

/// 递归收集 pxls 候选文件：
/// - root 是文件 → 直接收（后缀不限，交给提取器判断 raw/UnityFS）
/// - root 是目录 → 递归找 `*.pxls.dat` / `*.pxls` / `*.pxls.bytes`（含 .pxls.bytes 不含 texture_0）
pub fn collect_targets(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        return Err(format!("not a file or directory: {}", root.display()));
    }
    let mut out = Vec::new();
    walk(root, 0, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 12 {
        return Ok(()); // 防御性深度上限（StreamingAssets 层级很浅）
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if matches!(name.as_str(), ".git" | "target" | "node_modules" | "__pycache__") {
                continue;
            }
            walk(&p, depth + 1, out)?;
        } else if is_pxls_candidate(&p) {
            out.push(p);
        }
    }
    Ok(())
}

fn is_pxls_candidate(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.ends_with(".texture_0.dat") {
        return false; // 外部贴图分片，不是姿势表
    }
    name.ends_with(".pxls.dat") || name.ends_with(".pxls") || name.ends_with(".pxls.bytes")
}

/// 递归收集贴图包候选：root 是文件直接收；目录递归找 `*.texture_0.dat`
pub fn collect_tex_targets(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        return Err(format!("not a file or directory: {}", root.display()));
    }
    let mut out = Vec::new();
    walk_tex(root, 0, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_tex(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 12 {
        return Ok(());
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if matches!(name.as_str(), ".git" | "target" | "node_modules" | "__pycache__") {
                continue;
            }
            walk_tex(&p, depth + 1, out)?;
        } else {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if name.ends_with(".texture_0.dat") {
                out.push(p);
            }
        }
    }
    Ok(())
}

/// 极简 glob：`*` 任意串、`?` 单字符；不区分大小写。
pub fn glob_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    gm(&p, 0, &t, 0)
}

fn gm(p: &[char], mut pi: usize, t: &[char], mut ti: usize) -> bool {
    // 迭代 + 回溯的通配符匹配（避免深递归）
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, ti));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// 读取文件并提取 pxls 内容（raw / UnityFS 统一入口）
pub fn load_pxls(path: &Path) -> Result<crate::pxlslib::Pxls, String> {
    let d = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (_src, raw) = crate::unityfs::extract_pxls(&d)?;
    let mut p = crate::pxlslib::parse(&raw)?;
    crate::pxlslib::attach_img_sizes(&mut p);
    Ok(p)
}
