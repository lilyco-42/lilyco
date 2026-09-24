//! LetsGal 工程读写（project.json / characters.json / scenes.json / chapters/*.json / assets/.manifest.json）

use crate::{stable_id, uid, Story};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn init_project(dir: &Path, name: &str) -> Result<(), String> {
    for sub in ["chapters", "assets", "config"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
    }
    let project = json!({
        "id": uid(), "version": "1.0.0", "engineVersion": "1.0.0", "name": name,
        "resolution": {"width": 1920, "height": 1080}, "backgroundColor": "#000000",
        "window": {"lockAspectRatio": false, "allowMaximize": true, "allowFullscreen": true, "launchMode": "windowed"},
        "cursor": {"mode": "system", "image": "", "imagePixelRatio": 1,
            "imageSize": {"width": 24, "height": 24}, "hotspot": {"x": 0, "y": 0},
            "fallback": "default", "css": "", "cssImagePixelRatio": 1, "cssImageSize": {"width": 24, "height": 24}},
        "chapterOrder": [], "chapterFolders": [], "chapterTreeOrder": [],
        "extensionStorageVersion": 1, "extensions": {}, "extensionSettings": {},
        "dataBindings": {}, "dynamicVisualAssets": []
    });
    write_json(&dir.join("project.json"), &project)?;
    write_json(
        &dir.join("characters.json"),
        &json!({"version": 2, "globalSettings": {}, "attributeTemplate": [], "characters": []}),
    )?;
    write_json(
        &dir.join("scenes.json"),
        &json!({"version": 2, "scenes": []}),
    )?;
    write_json(
        &dir.join("assets/.manifest.json"),
        &json!({"version": 1, "entries": {}}),
    )?;
    Ok(())
}

pub fn write_chapters(dir: &Path, chapters: &[Value], name: Option<&str>) -> Result<(), String> {
    fs::create_dir_all(dir.join("chapters")).map_err(|e| e.to_string())?;
    for c in chapters {
        let fname = format!("{}.json", c["name"].as_str().unwrap_or("章"));
        write_json(&dir.join("chapters").join(fname), c)?;
    }
    // 更新 project.json
    let pp = dir.join("project.json");
    let mut project: Value = if pp.exists() {
        read_json(&pp)?
    } else {
        init_project(dir, name.unwrap_or("新游戏"))?;
        read_json(&pp)?
    };
    if let Some(n) = name {
        project["name"] = json!(n);
    }
    let order: Vec<Value> = chapters
        .iter()
        .map(|c| json!(c["name"].as_str().unwrap_or("")))
        .collect();
    let tree: Vec<Value> = chapters
        .iter()
        .map(|c| json!({"type": "chapter", "id": c["id"].as_str().unwrap_or("")}))
        .collect();
    project["chapterOrder"] = json!(order);
    project["chapterTreeOrder"] = json!(tree);
    write_json(&pp, &project)
}

pub fn upsert_character(dir: &Path, name: &str) -> Result<String, String> {
    let p = dir.join("characters.json");
    let mut data = if p.exists() {
        read_json(&p)?
    } else {
        json!({"version": 2, "globalSettings": {}, "attributeTemplate": [], "characters": []})
    };
    let chars = data["characters"]
        .as_array_mut()
        .ok_or("characters.json 格式错误")?;
    if let Some(existing) = chars.iter().find(|c| c["name"] == name) {
        return Ok(existing["id"].as_str().unwrap_or("").to_string());
    }
    let id = stable_id("letsgal-ai:character", name);
    chars.push(json!({"id": id, "name": name, "expressions": []}));
    write_json(&p, &data)?;
    Ok(id)
}

pub fn upsert_scene(dir: &Path, name: &str) -> Result<String, String> {
    let p = dir.join("scenes.json");
    let mut data = if p.exists() {
        read_json(&p)?
    } else {
        json!({"version": 2, "scenes": []})
    };
    let scenes = data["scenes"]
        .as_array_mut()
        .ok_or("scenes.json 格式错误")?;
    if let Some(existing) = scenes.iter().find(|s| s["name"] == name) {
        return Ok(existing["id"].as_str().unwrap_or("").to_string());
    }
    let id = stable_id("letsgal-ai:scene", name);
    scenes.push(json!({"id": id, "name": name, "layers": []}));
    write_json(&p, &data)?;
    Ok(id)
}

pub fn register_asset(dir: &Path, rel: &str) -> Result<Option<String>, String> {
    let abs = dir.join("assets").join(rel);
    if !abs.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&abs).map_err(|e| e.to_string())?;
    let hash = format!("{:x}", md5::compute(&bytes));
    let p = dir.join("assets/.manifest.json");
    let mut mf = if p.exists() {
        read_json(&p)?
    } else {
        json!({"version": 1, "entries": {}})
    };
    let entries = mf["entries"].as_object_mut().ok_or("manifest 格式错误")?;
    entries.retain(|_, v| v["path"].as_str() != Some(rel));
    entries.insert(hash.clone(), json!({"path": rel, "size": bytes.len(), "updatedAt": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64}));
    write_json(&p, &mf)?;
    Ok(Some(hash))
}

/// 校验：结构问题=issues；资产未登记=warnings
pub fn validate_project(dir: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let mut issues: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for rel in [
        "project.json",
        "characters.json",
        "scenes.json",
        "assets/.manifest.json",
    ] {
        if !dir.join(rel).exists() {
            issues.push(format!("missing {rel}"));
        }
    }
    if !dir.join("chapters").exists() {
        issues.push("missing chapters/".into());
    }
    let pp = dir.join("project.json");
    if pp.exists() {
        let project = read_json(&pp)?;
        if !project["chapterOrder"].is_array() {
            issues.push("project.json: chapterOrder 缺失".into());
        }
        if let Some(order) = project["chapterOrder"].as_array() {
            for ch in order {
                let f = dir
                    .join("chapters")
                    .join(format!("{}.json", ch.as_str().unwrap_or("")));
                if !f.exists() {
                    issues.push(format!(
                        "章节文件缺失: chapters/{}.json",
                        ch.as_str().unwrap_or("")
                    ));
                }
            }
        }
    }
    let manifest: Value = if dir.join("assets/.manifest.json").exists() {
        read_json(&dir.join("assets/.manifest.json"))?
    } else {
        json!({"entries": {}})
    };
    let asset_paths: Vec<String> = manifest["entries"]
        .as_object()
        .unwrap_or(&serde_json::Map::new())
        .values()
        .filter_map(|v| v["path"].as_str().map(|s| s.to_string()))
        .collect();
    let chapters_dir = dir.join("chapters");
    if chapters_dir.exists() {
        for entry in fs::read_dir(&chapters_dir).map_err(|e| e.to_string())? {
            let f = entry.map_err(|e| e.to_string())?.path();
            if f.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            let c = read_json(&f)?;
            if let Some(frags) = c["fragments"].as_array() {
                for fr in frags {
                    if let Some(blocks) = fr["blocks"].as_array() {
                        for b in blocks {
                            let t = b["type"].as_str().unwrap_or("");
                            if matches!(t, "scene" | "sound") {
                                if let Some(uri) = b["props"]["uri"].as_str() {
                                    if !uri.is_empty() && !asset_paths.contains(&uri.to_string()) {
                                        warnings.push(format!(
                                            "{}: 资产未在清单中: {uri}",
                                            f.file_name().unwrap_or_default().to_string_lossy()
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok((issues, warnings))
}

/// compile.js 的「无立绘不显形」后处理：没有任何表情素材的角色
/// （电话/录音/广播中的声音）不要求运行时显示不存在的立绘 ——
/// 其对白块 expression 清空、showCharacter/keepCharacter 置 false。
/// has_portrait 为 false 才改写（缺省/未知角色不动，与 Node 语义一致）。
pub fn apply_no_portrait_pass(chapters: &mut [Value], has_portrait: &HashMap<String, bool>) {
    for ch in chapters {
        if let Some(frags) = ch.get_mut("fragments").and_then(|f| f.as_array_mut()) {
            for f in frags {
                if let Some(blocks) = f.get_mut("blocks").and_then(|b| b.as_array_mut()) {
                    for b in blocks.iter_mut() {
                        if b["type"] != "dialogue" {
                            continue;
                        }
                        let name = b["props"]["characterName"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        if has_portrait.get(&name) == Some(&false) {
                            if let Some(props) = b["props"].as_object_mut() {
                                props.insert("expression".into(), json!(""));
                                props.insert("showCharacter".into(), json!(false));
                                props.insert("keepCharacter".into(), json!(false));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// DSL 解析树 → 完整工程（对齐 Node lib/compile.js compileStory 的落地段）：
/// upsert 角色/场景 + 以磁盘已有表情判定立绘 + 无立绘不显形 + 写章节。
/// 四端共用的唯一生产路径，测试也走它 —— 测的就是生产代码。
pub fn write_story_project(
    dir: &Path,
    story: &mut Story,
    name: Option<&str>,
) -> Result<(), String> {
    // 磁盘已有角色的表情状态（Studio 工程重编译时保留现有立绘）
    let mut has_portrait: HashMap<String, bool> = HashMap::new();
    let cp = dir.join("characters.json");
    if cp.exists() {
        let data: Value =
            serde_json::from_str(&fs::read_to_string(&cp).map_err(|e| e.to_string())?)
                .map_err(|e| format!("{}: {e}", cp.display()))?;
        if let Some(chars) = data["characters"].as_array() {
            for c in chars {
                let cname = c["name"].as_str().unwrap_or("").to_string();
                let has = c["expressions"].as_array().map_or(false, |a| !a.is_empty());
                has_portrait.insert(cname, has);
            }
        }
    }
    for c in &story.characters {
        let cname = c["name"].as_str().unwrap_or("").to_string();
        upsert_character(dir, &cname)?;
        has_portrait
            .entry(cname)
            .or_insert_with(|| c["expressions"].as_array().map_or(false, |a| !a.is_empty()));
    }
    for s in &story.scenes {
        upsert_scene(dir, s["name"].as_str().unwrap_or(""))?;
    }
    apply_no_portrait_pass(&mut story.chapters, &has_portrait);
    write_chapters(dir, &story.chapters, name)
}

fn read_json(p: &Path) -> Result<Value, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{}: {e}", p.display()))
}

fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        p,
        serde_json::to_string_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
