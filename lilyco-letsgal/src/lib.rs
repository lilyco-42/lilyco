//! lilyco-letsgal — 故事 DSL → LetsGal 工程（核心逻辑，与 Node 版 letsgal-ai 语义 1:1）
//! 格式参考：letsgal-ai 仓库 docs/letsgal-format.md（逆向自官方模板 + 1.11.0 安装包）
#![recursion_limit = "512"]

pub mod dsl;
pub mod project;

pub use dsl::{parse_story, Story};
pub use project::{
    init_project, register_asset, upsert_character, upsert_scene, validate_project, write_chapters,
};

use serde_json::{json, Value};

/// 由名称生成稳定 UUID（重编译不漂移），与 Node 版 stableId 一致
pub fn stable_id(namespace: &str, name: &str) -> String {
    let digest = md5::compute(format!("{namespace}:{name}"));
    let h = format!("{:x}", digest);
    format!(
        "{}-{}-4{}-8{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    )
}

pub(crate) fn uid() -> String {
    // 无 uuid 依赖：时间戳 + 随机后缀
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("frag-{t}-{}", rand_suffix())
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    (t % 1_000_000) as u64
}

// ---------- 块构建器（对齐 Node lib/blocks.js） ----------

pub fn curtain(op: &str, duration: &str) -> Value {
    json!({"id": uid(), "type": "curtain", "props": {"disabled": false, "op": op, "effect": "", "duration": duration, "color": "#000000", "mode": "full-screen", "curtainSize": "100"}})
}

pub fn scene(scene_id: &str, scene_name: &str, uri: &str, td: &str) -> Value {
    json!({"id": uid(), "type": "scene", "props": {
        "disabled": false, "sceneId": scene_id, "sceneName": scene_name, "uri": uri,
        "transitionMode": "cover", "transitionDuration": td, "transitionDirection": "", "transitionStrips": "12",
        "transitionRuleUri": "", "transitionCenterX": "50", "transitionCenterY": "50", "transitionSoftness": "",
        "transitionZoomScale": "1.14", "transitionBlurStrength": "16", "transitionMosaicSize": "52",
        "transitionGlitchStrength": "100", "transitionGlitchColorShift": "4.5", "transitionGlitchScanlines": "17",
        "transitionPagePerspective": "50", "transitionPageShadow": "38", "waitForComplete": "false",
        "resetCamera": "false", "displayType": "cover", "position": "(50%,50%)", "anchor": "center", "size": ""
    }})
}

pub fn particle(mode: &str, preset: &str, texture_uri: &str) -> Value {
    json!({"id": uid(), "type": "particle", "props": {
        "disabled": false, "mode": mode, "effectId": uid(), "preset": preset, "textureUri": texture_uri,
        "optionsJson": "", "fadeInDuration": "500", "fadeOutDuration": "500"
    }})
}

pub fn camera(zoom: &str, dur: &str) -> Value {
    json!({"id": uid(), "type": "camera", "props": {
        "disabled": false, "offsetX": "", "offsetY": "", "zoom": zoom, "focalDistance": "", "blurStrength": "",
        "duration": dur, "easing": "easeInOut", "waitForComplete": "true", "tweenFields": "offsetX,offsetY,zoom",
        "targets": "scene,characters", "objectTargetsJson": "", "targetSelectionPending": "",
        "distortionStrength": "", "vignetteIntensity": "", "vignetteSize": "", "blurAmount": "",
        "colorToneMode": "none", "colorToneIntensity": "", "colorExposure": "", "colorBrightness": "",
        "colorContrast": "", "colorSaturation": "", "colorTemperature": "", "oldFilmIntensity": "",
        "shockIntensity": "", "godrayIntensity": "", "godrayAngle": "", "godrayGain": "", "godrayLacunarity": "",
        "godraySpeed": "", "godrayParallel": "", "godrayCenterX": "", "godrayCenterY": "", "lutPreset": "",
        "lutIntensity": "", "bloomIntensity": "", "chromaticAberration": "", "pixelateSize": "", "glitchIntensity": "",
        "crtIntensity": "", "sharpenStrength": "", "radialBlurStrength": "", "radialBlurCenterX": "", "radialBlurCenterY": "",
        "motionBlurStrength": "", "motionBlurAngle": "", "zoomBlurStrength": "", "zoomBlurCenterX": "", "zoomBlurCenterY": "",
        "lightLeakIntensity": "", "lightLeakAngle": "", "lensFlareIntensity": "", "lensFlareCenterX": "", "lensFlareCenterY": "",
        "filmGrainIntensity": "", "filmGrainSize": "", "heatHazeIntensity": "", "heatHazeSpeed": "", "heatHazeScale": "",
        "waterRippleIntensity": "", "waterRippleFrequency": "", "waterRippleSpeed": "", "waterRippleCenterX": "", "waterRippleCenterY": "",
        "fogIntensity": "", "fogSpeed": "", "fogScale": "", "vhsIntensity": "", "vhsJitter": "", "vhsNoise": "",
        "halftoneIntensity": "", "halftoneScale": "", "halftoneAngle": "", "ditherIntensity": "", "ditherLevels": "",
        "outlineIntensity": "", "outlineThickness": "", "eyelidOpenness": "", "eyelidWidth": "", "eyelidCurvature": "",
        "eyelidSoftness": "", "eyelidCenterX": "", "eyelidCenterY": "", "shakeAmplitude": "", "shakeFrequency": "",
        "shakeDuration": "", "shakeFalloff": "linear", "shakeAxis": "both"
    }})
}

pub fn sound(sound_type: &str, uri: &str, volume: &str, loop_flag: bool) -> Value {
    json!({"id": uid(), "type": "sound", "props": {
        "disabled": false, "soundType": sound_type, "soundId": "", "uri": uri,
        "volume": volume, "loop": if loop_flag { "true" } else { "false" }, "fadeDuration": ""
    }})
}

pub fn stop_sound(sound_type: &str, fade: &str) -> Value {
    json!({"id": uid(), "type": "stopSound", "props": {"disabled": false, "soundType": sound_type, "soundId": "", "fadeDuration": fade}})
}

pub fn wait(ms: u64) -> Value {
    json!({"id": uid(), "type": "wait", "props": {"disabled": false, "duration": ms, "waitForInput": false}})
}

pub fn narration(text: &str) -> Value {
    json!({"id": uid(), "type": "narration", "props": {"disabled": false, "keepDialogue": false, "voiceHash": ""},
           "content": [{"type": "text", "text": text, "styles": {}}]})
}

pub fn dialogue(char_id: &str, char_name: &str, text: &str, expr: &str) -> Value {
    json!({"id": uid(), "type": "dialogue", "props": {
        "disabled": false, "characterId": char_id, "characterName": char_name, "nameVariantId": "", "expression": expr,
        "skin": "", "position": "", "isFirst": true, "isLast": true, "prevExpression": "", "prevNameVariantId": "",
        "showCharacter": true, "keepCharacter": true, "keepDialogue": true, "voiceHash": ""
    }, "content": [{"type": "text", "text": text, "styles": {}}]})
}

pub fn comment(text: &str) -> Value {
    json!({"id": uid(), "type": "comment", "props": {"disabled": false}, "content": [{"type": "text", "text": text, "styles": {}}]})
}

pub fn floating_text(text: &str, font_size: &str, dur: &str) -> Value {
    json!({"id": uid(), "type": "floatingText", "props": {
        "disabled": false, "position": "(50%,50%)", "anchor": "center", "fontSize": font_size, "color": "#ffffff",
        "fontWeight": "", "textShadow": "0 2px 8px rgba(0,0,0,0.6)", "duration": dur,
        "animIn": "fade", "inDuration": "800", "animOut": "fade", "outDuration": "800",
        "slideFrom": "top", "slideDistance": "0", "scaleFrom": "1", "scaleTo": "1", "blurRadius": ""
    }, "content": [{"type": "text", "text": text, "styles": {}}]})
}

pub fn call_fragment(fragment_id: &str) -> Value {
    json!({"id": uid(), "type": "callFragment", "props": {"disabled": false, "fragmentId": fragment_id}})
}

pub fn show_title_ui() -> Value {
    json!({"id": uid(), "type": "showExtensionUI", "props": {
        "disabled": false, "target": "slot:internal.system.title", "modal": true, "layer": "topmost",
        "seekBehavior": "skip-if-seeking", "size": "", "position": "", "interactable": ""
    }})
}

pub fn branch(branch_id: &str, options_json: Value) -> Value {
    json!({"id": uid(), "type": "branch", "props": {"disabled": false, "branchId": branch_id, "optionsJson": serde_json::to_string(&options_json).unwrap_or_default()}})
}

pub fn return_to_entry() -> Value {
    json!({"id": uid(), "type": "returnToEntry", "props": {"disabled": false}})
}

/// 角色流后处理：同场景持续在场，换人/硬边界退场（修复叠台 bug）
pub fn apply_character_flow(chapters: &mut [Value]) {
    const BOUNDARY: &[&str] = &[
        "scene",
        "curtain",
        "destroyScene",
        "callFragment",
        "branch",
        "returnToEntry",
        "showExtensionUI",
        "removeCharacter",
        "stopSound",
    ];
    for ch in chapters {
        if let Some(frags) = ch.get_mut("fragments").and_then(|f| f.as_array_mut()) {
            for f in frags {
                let blocks = match f.get_mut("blocks").and_then(|b| b.as_array_mut()) {
                    Some(b) => b,
                    None => continue,
                };
                for i in 0..blocks.len() {
                    if blocks[i]["type"] != "dialogue" {
                        continue;
                    }
                    let char_id = blocks[i]["props"]["characterId"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let mut keep = false;
                    for nb in blocks.iter().skip(i + 1) {
                        if nb["type"] == "dialogue" {
                            keep = nb["props"]["characterId"].as_str() == Some(char_id.as_str());
                            break;
                        }
                        if BOUNDARY.contains(&nb["type"].as_str().unwrap_or("")) {
                            break;
                        }
                    }
                    if let Some(props) = blocks[i]["props"].as_object_mut() {
                        props.insert("keepCharacter".into(), json!(keep));
                        props.insert("keepDialogue".into(), json!(keep));
                    }
                }
            }
        }
    }
}
