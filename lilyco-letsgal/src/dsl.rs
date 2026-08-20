//! 故事 DSL 解析器（与 letsgal-ai Node 版 lib/story.js 语义一致）
//! 规范见 letsgal-ai 仓库 docs/STORY-DSL.md

use crate::{
    apply_character_flow, branch, call_fragment, camera, comment, curtain, dialogue, floating_text,
    narration, particle, return_to_entry, scene, show_title_ui, sound, stable_id, stop_sound, uid,
    wait,
};
use serde_json::{json, Value};

pub struct Story {
    pub chapters: Vec<Value>,
    pub characters: Vec<Value>,
    pub scenes: Vec<Value>,
}

const CHAR_NS: &str = "letsgal-ai:character";
const SCENE_NS: &str = "letsgal-ai:scene";

/// 解析一行 `角色(表情)：台词` / `旁白：…`
enum LineKind {
    Narration(String),
    Dialogue {
        char: String,
        expr: String,
        text: String,
    },
}

fn parse_kv_line(s: &str) -> Option<LineKind> {
    let (head, text) = s.split_once([':', '：'])?;
    let head = head.trim();
    let text = text.trim().to_string();
    if matches!(head, "旁白" | "心理" | "n" | "narration") {
        return Some(LineKind::Narration(text));
    }
    if let Some((name, expr)) = head.rsplit_once(['（', '(']).and_then(|(a, b)| {
        b.strip_suffix(['）', ')'])
            .map(|e| (a.trim().to_string(), e.trim().to_string()))
    }) {
        return Some(LineKind::Dialogue {
            char: name,
            expr,
            text,
        });
    }
    Some(LineKind::Dialogue {
        char: head.to_string(),
        expr: String::new(),
        text,
    })
}

pub fn parse_story(text: &str) -> Story {
    let mut chapters: Vec<Value> = Vec::new();
    let mut characters: Vec<Value> = Vec::new();
    let mut scenes: Vec<Value> = Vec::new();
    let mut cur: Option<usize> = None; // chapters 下标
    let mut frag: Option<usize> = None; // fragments 下标

    let ensure_frag = |chapters: &mut Vec<Value>, cur: Option<usize>, frag: &mut Option<usize>| {
        if frag.is_none() {
            if let Some(ci) = cur {
                if let Some(frags) = chapters[ci]
                    .get_mut("fragments")
                    .and_then(|f| f.as_array_mut())
                {
                    frags.push(json!({"id": uid(), "name": "main", "blocks": []}));
                    *frag = Some(frags.len() - 1);
                }
            }
        }
    };
    let push_block =
        |chapters: &mut Vec<Value>, cur: Option<usize>, frag: Option<usize>, b: Value| {
            if let (Some(ci), Some(fi)) = (cur, frag) {
                if let Some(blocks) = chapters[ci]["fragments"][fi]
                    .get_mut("blocks")
                    .and_then(|x| x.as_array_mut())
                {
                    blocks.push(b);
                }
            }
        };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(name) = line.strip_prefix("## ") {
            if let Some(ci) = cur {
                if let Some(frags) = chapters[ci]
                    .get_mut("fragments")
                    .and_then(|f| f.as_array_mut())
                {
                    frags.push(json!({"id": uid(), "name": name.trim(), "blocks": []}));
                    frag = Some(frags.len() - 1);
                }
            }
            continue;
        }
        if let Some(name) = line.strip_prefix('#') {
            chapters.push(json!({"id": uid(), "name": name.trim(), "fragments": []}));
            cur = Some(chapters.len() - 1);
            frag = None;
            ensure_frag(&mut chapters, cur, &mut frag);
            continue;
        }
        if cur.is_none() {
            chapters.push(json!({"id": uid(), "name": "序章", "fragments": []}));
            cur = Some(chapters.len() - 1);
            ensure_frag(&mut chapters, cur, &mut frag);
        }

        if let Some(rest) = line.strip_prefix('!') {
            let mut parts = rest.split_whitespace();
            let cmd = parts.next().unwrap_or("").to_string();
            let mut kv: Vec<(String, String)> = Vec::new();
            let mut pos: Vec<String> = Vec::new();
            for a in parts {
                if let Some((k, v)) = a.split_once('=') {
                    kv.push((k.to_string(), v.to_string()));
                } else if a == "loop" {
                    kv.push(("loop".into(), "true".into()));
                } else {
                    pos.push(a.to_string());
                }
            }
            let arg = pos.join(" ");
            let get = |k: &str| kv.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone());
            ensure_frag(&mut chapters, cur, &mut frag);
            let b: Option<Value> = match cmd.as_str() {
                "curtain" => Some(curtain(
                    if arg.is_empty() { "close" } else { &arg },
                    &get("dur")
                        .or(get("duration"))
                        .unwrap_or_else(|| "1000".into()),
                )),
                "scene" | "cg" => {
                    let mut it = arg.split_whitespace();
                    let sname = it.next().unwrap_or("场景").to_string();
                    let suri = it.next().unwrap_or("").to_string();
                    let sid = stable_id(SCENE_NS, &sname);
                    scenes.push(json!({"id": sid, "name": sname}));
                    Some(scene(
                        &sid,
                        &sname,
                        &suri,
                        &get("td").unwrap_or_else(|| "500".into()),
                    ))
                }
                "bgm" => Some(sound(
                    "BGM",
                    &arg,
                    &get("vol").unwrap_or_else(|| "45".into()),
                    get("loop").is_some(),
                )),
                "se" => Some(sound(
                    "SE",
                    &arg,
                    &get("vol").unwrap_or_else(|| "60".into()),
                    get("loop").is_some(),
                )),
                "voice" => Some(sound(
                    "VOICE",
                    &arg,
                    &get("vol").unwrap_or_else(|| "80".into()),
                    false,
                )),
                "stopbgm" => Some(stop_sound(
                    "BGM",
                    &get("fade").unwrap_or_else(|| "1000".into()),
                )),
                "stopse" => Some(stop_sound(
                    "SE",
                    &get("fade").unwrap_or_else(|| "500".into()),
                )),
                "particle" => {
                    let mut it = arg.split_whitespace();
                    let preset = it.next().unwrap_or("LIGHT_SNOW").to_string();
                    let uri = it.next().unwrap_or("particles/snow.png").to_string();
                    Some(particle("show", &preset, &uri))
                }
                "camera" => Some(camera(
                    &get("zoom").unwrap_or_default(),
                    &get("dur").or(get("duration")).unwrap_or_else(|| "0".into()),
                )),
                "wait" => Some(wait(arg.parse().unwrap_or(800))),
                "float" => Some(floating_text(
                    &arg,
                    &get("size").unwrap_or_else(|| "42".into()),
                    &get("dur").unwrap_or_else(|| "2000".into()),
                )),
                "comment" => Some(comment(&arg)),
                "title" => Some(show_title_ui()),
                "end" => Some(return_to_entry()),
                "call" => Some(call_fragment(&get("id").unwrap_or(arg))),
                "choice" => {
                    let opts: Vec<Value> = arg.split('|').filter_map(|o| {
                        let (t, target) = o.split_once("->")?;
                        Some(json!({"mode": "jump", "text": t.trim(), "fragmentId": target.trim()}))
                    }).collect();
                    Some(branch(&uid(), json!(opts)))
                }
                _ => Some(comment(&format!("[未知指令 {cmd}] {arg}"))),
            };
            if let Some(b) = b {
                push_block(&mut chapters, cur, frag, b);
            }
            continue;
        }

        // 文本行
        ensure_frag(&mut chapters, cur, &mut frag);
        if line.starts_with('（') || line.starts_with('(') {
            push_block(&mut chapters, cur, frag, narration(line));
            continue;
        }
        match parse_kv_line(line) {
            None => push_block(&mut chapters, cur, frag, narration(line)),
            Some(LineKind::Narration(t)) => push_block(&mut chapters, cur, frag, narration(&t)),
            Some(LineKind::Dialogue { char, expr, text }) => {
                if !characters.iter().any(|c| c["name"] == char) {
                    characters.push(
                        json!({"id": stable_id(CHAR_NS, &char), "name": char, "expressions": []}),
                    );
                }
                let cid = stable_id(CHAR_NS, &char);
                push_block(
                    &mut chapters,
                    cur,
                    frag,
                    dialogue(&cid, &char, &text, &expr),
                );
            }
        }
    }

    // 二次解析：choice/call 的 fragmentId 按名称解析
    let mut frag_id_by_name: Vec<(String, String)> = Vec::new();
    for c in &chapters {
        if let Some(frags) = c["fragments"].as_array() {
            for f in frags {
                if let (Some(n), Some(id)) = (f["name"].as_str(), f["id"].as_str()) {
                    frag_id_by_name.push((n.to_string(), id.to_string()));
                }
            }
        }
    }
    for ch in chapters.iter_mut() {
        if let Some(frags) = ch["fragments"].as_array_mut() {
            for f in frags {
                if let Some(blocks) = f["blocks"].as_array_mut() {
                    for b in blocks.iter_mut() {
                        if b["type"] == "branch" {
                            let mut opts: Vec<Value> = serde_json::from_str(
                                b["props"]["optionsJson"].as_str().unwrap_or("[]"),
                            )
                            .unwrap_or_default();
                            for o in opts.iter_mut() {
                                let fid = o["fragmentId"].as_str().unwrap_or("").to_string();
                                if !fid.starts_with("frag-") {
                                    if let Some((_, real)) =
                                        frag_id_by_name.iter().find(|(n, _)| *n == fid)
                                    {
                                        o["fragmentId"] = json!(real);
                                    }
                                }
                            }
                            b["props"]["optionsJson"] =
                                json!(serde_json::to_string(&opts).unwrap_or_default());
                        }
                        if b["type"] == "callFragment" {
                            let fid = b["props"]["fragmentId"].as_str().unwrap_or("").to_string();
                            if !fid.starts_with("frag-") {
                                if let Some((_, real)) =
                                    frag_id_by_name.iter().find(|(n, _)| *n == fid)
                                {
                                    b["props"]["fragmentId"] = json!(real);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    apply_character_flow(&mut chapters);
    Story {
        chapters,
        characters,
        scenes,
    }
}
