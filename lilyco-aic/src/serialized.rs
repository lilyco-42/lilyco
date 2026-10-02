//! SerializedFile（Unity 内层序列化文件）v22 解析 + typetree 驱动的通用对象字段提取。
//!
//! 本作 StreamingAssets 的 `*.texture_0.dat` 是 UnityFS 包，内层为一份 v22 SerializedFile
//! （Unity 2022.3.62f2，StandaloneWindows64），含 Texture2D 对象与 `archive:/*.resS` 资源流节点。
//!
//! ## 格式（实测 noel.pxls.bytes.texture_0.dat 逐字节标定，与 UnityPy 交叉验证）
//!
//! 头 48 字节，全部大端：
//! `metadata_size u32(0) + file_size u32(0) + version u32(22) + data_offset u32(0)`
//! `+ endian 字节(0=LE) + 3 保留字节 + metadata_size u32 + file_size u64 + data_offset u64 + unknown u64`
//!
//! 元数据（按 endian 字节切换端序，本作为 LE）：
//! unity 版本串(null 结尾) → target_platform i32 → enable_type_tree bool → 类型表 → 对象表
//!
//! 类型表每项：class_id i32 + stripped bool(v≥16) + script_type_index i16(v≥17)
//! + script_id 16B(仅 class_id==114) + old_type_hash 16B
//! + typetree blob(若 enable_type_tree 且未 stripped)：node_count i32 + stringbuffer_size i32
//!   + node_count × 32B 节点(v≥19：`i16 ver, u8 level, u8 typeflags, u32 typeStr, u32 nameStr, i32 byteSize, i32 index, i32 metaFlag, u64 refHash`) + stringbuffer
//! + type_dependencies i32 数组(v≥21)
//!
//! 对象表每项：align4 → path_id i64 → byte_start u64(+data_offset) → byte_size u32 → type_id i32
//!
//! ## typetree 驱动解析
//!
//! 对象字节按类型树逐字段走；对齐**不是**猜的——blob 节点的 MetaFlag 第 14 位(0x4000)
//! 是 AlignBytes 标志，读完后 align4。string = `u32 len(不含 null) + len 字节 + align`；
//! vector/TypelessData = `u32 count + 数据 + align(按 Array 子节点标志)`。

use std::collections::BTreeMap;

/// typetree 节点（blob 32B 解出）
#[derive(Debug, Clone)]
pub struct TtNode {
    pub level: i16,
    pub type_name: String,
    pub field_name: String,
    #[allow(dead_code)]
    pub byte_size: i32,
    pub meta_flag: u32,
}

/// typetree 驱动解出的字段值
#[derive(Debug, Clone)]
#[allow(dead_code)] // Flt/Bool/Arr 供后续姿势写回（TextAsset m_Script）使用
pub enum TyVal {
    Int(i64),
    Flt(f32),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Arr(Vec<TyVal>),
    Obj(BTreeMap<String, TyVal>),
}

impl TyVal {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            TyVal::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TyVal::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_obj(&self) -> Option<&BTreeMap<String, TyVal>> {
        match self {
            TyVal::Obj(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            TyVal::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// SerializedFile 内的一个对象引用
#[derive(Debug, Clone)]
pub struct SfObject {
    pub path_id: i64,
    /// 已加 data_offset，相对解压后数据区的绝对位置
    pub byte_start: usize,
    pub byte_size: usize,
    pub type_id: u32,
    pub class_name: String,
}

/// 内层 SerializedFile
#[allow(dead_code)] // unity_version/target_platform 供诊断输出，find_by_class 供写回路径使用
pub struct SerializedFile {
    pub unity_version: String,
    pub target_platform: i32,
    pub objects: Vec<SfObject>,
    /// type_id → typetree（None = stripped/无树）
    pub type_trees: Vec<Option<Vec<TtNode>>>,
}

const META_ALIGN: u32 = 0x4000;

impl SerializedFile {
    /// 从字节解析（输入 = UnityFS 节点切出的 SerializedFile 数据）
    pub fn parse(d: &[u8]) -> Result<SerializedFile, String> {
        if d.len() < 48 {
            return Err(format!("SerializedFile too short: {} bytes", d.len()));
        }
        // 头 16 字节占位（BE）：metadata_size, file_size, version, data_offset
        let version = u32::from_be_bytes(d[8..12].try_into().unwrap());
        let endian_le = d[16] == 0;
        if !endian_le {
            return Err("big-endian SerializedFile not supported".into());
        }
        if version < 22 {
            return Err(format!("SerializedFile version {version} < 22 not supported"));
        }
        // v22 真头（BE）：metadata_size u32 + file_size u64 + data_offset u64 + unknown u64
        // d[17..21] metadata_size；d[21..29] file_size；d[29..37]? —— 不对，v22 布局是
        // endian(1)+reserved(3) 在 @16..20，然后 metadata_size @20..24, file_size @24..32,
        // data_offset @32..40, unknown @40..48。实测 noel：data_offset @32..40。
        let data_offset = u64::from_be_bytes(d[32..40].try_into().unwrap()) as usize;

        let mut r = Le::new(d);
        // v22 头 48 字节（BE）：16B 占位 + endian(1) + reserved(3) + metadata_size(4)
        // + file_size(8) + data_offset(8) + unknown(8)；元数据从 @48 开始（LE）
        r.pos = 48;
        let unity_version = r.cstring()?;
        let target_platform = r.i32()?;
        let enable_type_tree = r.u8()? != 0;
        let type_count = r.u32()? as usize;

        let mut type_trees: Vec<Option<Vec<TtNode>>> = Vec::with_capacity(type_count);
        for _ in 0..type_count {
            let class_id = r.i32()?;
            let stripped = r.u8()? != 0; // v>=16
            let _script_type_index = r.i16()?; // v>=17
            if class_id == 114 {
                r.skip(16)?; // script_id（本作 bundle 内不会出现 MonoBehaviour，防御性跳过）
            }
            r.skip(16)?; // old_type_hash
            if enable_type_tree && !stripped {
                let node_count = r.u32()? as usize;
                let sb_size = r.u32()? as usize;
                let mut raws: Vec<(i16, u8, u32, u32, i32, u32)> = Vec::with_capacity(node_count);
                for _ in 0..node_count {
                    // v>=19 blob node 32B：i16 version, u8 level, u8 type_flags,
                    // u32 type_str_off, u32 name_str_off, i32 byte_size, i32 index,
                    // i32 meta_flag, u64 ref_hash
                    // （注意：UnityPy keys 里第一个 h 是 m_Version，m_Level 是第一个 u8！）
                    let _version = r.i16()?;
                    let level = r.u8()? as i16;
                    let type_flags = r.u8()?;
                    let type_str_off = r.u32()?;
                    let name_str_off = r.u32()?;
                    let byte_size = r.i32()?;
                    let _index = r.i32()?;
                    let meta_flag = r.u32()?;
                    r.skip(8)?; // refHash (v>=19)
                    raws.push((level, type_flags, type_str_off, name_str_off, byte_size, meta_flag));
                }
                let sb = r.take(sb_size)?;
                let nodes: Vec<TtNode> = raws
                    .into_iter()
                    .map(|(level, _tf, tso, nso, byte_size, meta)| TtNode {
                        level,
                        type_name: string_from_buffer(sb, tso),
                        field_name: string_from_buffer(sb, nso),
                        byte_size,
                        meta_flag: meta,
                    })
                    .collect();
                // v>=21: type_dependencies i32 数组
                let dep_count = r.u32()?;
                let dep_count = dep_count as usize;
                r.skip(dep_count * 4)?;
                type_trees.push(Some(nodes));
            } else {
                type_trees.push(None);
            }
        }

        // 对象表
        let object_count = r.u32()? as usize;
        let mut objects = Vec::with_capacity(object_count);
        for _ in 0..object_count {
            r.align4();
            let path_id = r.i64()?;
            let byte_start = r.u64()? as usize + data_offset;
            let byte_size = r.u32()? as usize;
            let type_id = r.u32()?;
            let class_name = type_trees
                .get(type_id as usize)
                .and_then(|t| t.as_ref())
                .and_then(|n| n.first())
                .map(|root| root.type_name.clone())
                .unwrap_or_else(|| "Class?".to_string());
            objects.push(SfObject {
                path_id,
                byte_start,
                byte_size,
                type_id,
                class_name,
            });
        }

        Ok(SerializedFile {
            unity_version,
            target_platform,
            objects,
            type_trees,
        })
    }

    /// 找第一个指定 class 的对象
    #[allow(dead_code)] // 测试 + 二期姿势写回（定位 TextAsset）使用
    pub fn find_by_class(&self, name: &str) -> Option<&SfObject> {
        self.objects.iter().find(|o| o.class_name == name)
    }

    /// typetree 驱动解析对象字节 → 顶层字段表
    pub fn read_object(
        &self,
        o: &SfObject,
        d: &[u8],
    ) -> Result<BTreeMap<String, TyVal>, String> {
        let nodes = self
            .type_trees
            .get(o.type_id as usize)
            .and_then(|t| t.as_ref())
            .ok_or_else(|| format!("path_id {} has no typetree", o.path_id))?;
        let raw = d
            .get(o.byte_start..o.byte_start + o.byte_size)
            .ok_or_else(|| format!("object range {}+{} out of file", o.byte_start, o.byte_size))?;
        let mut rd = Le::new(raw);
        let mut out = BTreeMap::new();
        drive(&mut rd, nodes, 1, 1, &mut out)?;
        Ok(out)
    }
}

/// stringbuffer 里的字符串：offset 高位为 0 时是 buffer 内偏移（null 结尾）；
/// 高位为 1 时是 Unity 内置 CommonString 表（int/bool 等基础类型名，本作用不到，返回占位）。
fn string_from_buffer(sb: &[u8], off: u32) -> String {
    if off & 0x8000_0000 != 0 {
        return common_string(off & 0x7FFF_FFFF);
    }
    let off = off as usize;
    if off >= sb.len() {
        return String::new();
    }
    let end = sb[off..]
        .iter()
        .position(|&c| c == 0)
        .map(|e| off + e)
        .unwrap_or(sb.len());
    String::from_utf8_lossy(&sb[off..end]).into_owned()
}

/// Unity CommonString 内置表（114 条，与 UnityPy TypeTreeHelper.get_common_strings 一致）。
/// typetree blob 节点的字符串偏移高位为 1 时指向此表。
fn common_string(off: u32) -> String {
    let s = match off {
        0 => "AABB",
        5 => "AnimationClip",
        19 => "AnimationCurve",
        34 => "AnimationState",
        49 => "Array",
        55 => "Base",
        60 => "BitField",
        69 => "bitset",
        76 => "bool",
        81 => "char",
        86 => "ColorRGBA",
        96 => "Component",
        106 => "data",
        111 => "deque",
        117 => "double",
        124 => "dynamic_array",
        138 => "FastPropertyName",
        155 => "first",
        161 => "float",
        167 => "Font",
        172 => "GameObject",
        183 => "Generic Mono",
        196 => "GradientNEW",
        208 => "GUID",
        213 => "GUIStyle",
        222 => "int",
        226 => "list",
        231 => "long long",
        241 => "map",
        245 => "Matrix4x4f",
        256 => "MdFour",
        263 => "MonoBehaviour",
        277 => "MonoScript",
        288 => "m_ByteSize",
        299 => "m_Curve",
        307 => "m_EditorClassIdentifier",
        331 => "m_EditorHideFlags",
        349 => "m_Enabled",
        359 => "m_ExtensionPtr",
        374 => "m_GameObject",
        387 => "m_Index",
        395 => "m_IsArray",
        405 => "m_IsStatic",
        416 => "m_MetaFlag",
        427 => "m_Name",
        434 => "m_ObjectHideFlags",
        452 => "m_PrefabInternal",
        469 => "m_PrefabParentObject",
        490 => "m_Script",
        499 => "m_StaticEditorFlags",
        519 => "m_Type",
        526 => "m_Version",
        536 => "Object",
        543 => "pair",
        548 => "PPtr<Component>",
        564 => "PPtr<GameObject>",
        581 => "PPtr<Material>",
        596 => "PPtr<MonoBehaviour>",
        616 => "PPtr<MonoScript>",
        633 => "PPtr<Object>",
        646 => "PPtr<Prefab>",
        659 => "PPtr<Sprite>",
        672 => "PPtr<TextAsset>",
        688 => "PPtr<Texture>",
        702 => "PPtr<Texture2D>",
        718 => "PPtr<Transform>",
        734 => "Prefab",
        741 => "Quaternionf",
        753 => "Rectf",
        759 => "RectInt",
        767 => "RectOffset",
        778 => "second",
        785 => "set",
        789 => "short",
        795 => "size",
        800 => "SInt16",
        807 => "SInt32",
        814 => "SInt64",
        821 => "SInt8",
        827 => "staticvector",
        840 => "string",
        847 => "TextAsset",
        857 => "TextMesh",
        866 => "Texture",
        874 => "Texture2D",
        884 => "Transform",
        894 => "TypelessData",
        907 => "UInt16",
        914 => "UInt32",
        921 => "UInt64",
        928 => "UInt8",
        934 => "unsigned int",
        947 => "unsigned long long",
        966 => "unsigned short",
        981 => "vector",
        988 => "Vector2f",
        997 => "Vector3f",
        1006 => "Vector4f",
        1015 => "m_ScriptingClassIdentifier",
        1042 => "Gradient",
        1051 => "Type*",
        1057 => "int2_storage",
        1070 => "int3_storage",
        1083 => "BoundsInt",
        1093 => "m_CorrespondingSourceObject",
        1121 => "m_PrefabInstance",
        1138 => "m_PrefabAsset",
        1152 => "FileSize",
        1161 => "Hash128",
        1169 => "RenderingLayerMask",
        1188 => "fixed_array",
        1200 => "EntityId",
        1209 => "LoadableObjectId",
        1226 => "LoadableSceneId",
        _ => return format!("<common:{off:#x}>"),
    };
    s.to_string()
}

/// LE 读取器（SerializedFile 元数据与对象数据均为 LE），内部持有位置
pub struct Le<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Le<'a> {
    pub fn new(d: &'a [u8]) -> Le<'a> {
        Le { d, pos: 0 }
    }
    fn need(&self, n: usize) -> Result<(), String> {
        if self.pos + n > self.d.len() {
            return Err(format!(
                "eof: need {n} at {} of {}",
                self.pos,
                self.d.len()
            ));
        }
        Ok(())
    }
    pub fn u8(&mut self) -> Result<u8, String> {
        self.need(1)?;
        let v = self.d[self.pos];
        self.pos += 1;
        Ok(v)
    }
    pub fn i16(&mut self) -> Result<i16, String> {
        self.need(2)?;
        let v = i16::from_le_bytes(self.d[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }
    pub fn i32(&mut self) -> Result<i32, String> {
        self.need(4)?;
        let v = i32::from_le_bytes(self.d[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.d[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }
    pub fn i64(&mut self) -> Result<i64, String> {
        self.need(8)?;
        let v = i64::from_le_bytes(self.d[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }
    pub fn u64(&mut self) -> Result<u64, String> {
        self.need(8)?;
        let v = u64::from_le_bytes(self.d[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }
    pub fn f32(&mut self) -> Result<f32, String> {
        self.need(4)?;
        let v = f32::from_le_bytes(self.d[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }
    /// null 结尾字符串
    pub fn cstring(&mut self) -> Result<String, String> {
        let start = self.pos;
        while self.pos < self.d.len() && self.d[self.pos] != 0 {
            self.pos += 1;
        }
        let s = String::from_utf8_lossy(&self.d[start..self.pos]).into_owned();
        self.pos += 1; // null
        Ok(s)
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.need(n)?;
        let s = &self.d[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn skip(&mut self, n: usize) -> Result<(), String> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }
    pub fn align4(&mut self) {
        self.pos = self.pos.div_ceil(4) * 4;
    }
}

/// typetree 驱动解析：从节点 i 开始消费 level>=lvl 的字段序列。
/// 对齐规则：读完后按该节点 MetaFlag 的 0x4000 位 align4；
/// string/vector/TypelessData 的对齐标志挂在 Array 子节点上。
pub fn drive(
    rd: &mut Le,
    nodes: &[TtNode],
    i: usize,
    lvl: i16,
    out: &mut BTreeMap<String, TyVal>,
) -> Result<usize, String> {
    let mut i = i;
    while i < nodes.len() {
        let n = &nodes[i];
        if n.level < lvl {
            return Ok(i);
        }
        match n.type_name.as_str() {
            "SInt32" | "int" | "signed int" => {
                let v = rd.i32()?;
                if n.meta_flag & META_ALIGN != 0 {
                    rd.align4();
                }
                out.insert(n.field_name.clone(), TyVal::Int(v as i64));
                i += 1;
            }
            "UInt32" | "unsigned int" | "FileSize" => {
                let v = rd.u32()?;
                if n.meta_flag & META_ALIGN != 0 {
                    rd.align4();
                }
                out.insert(n.field_name.clone(), TyVal::Int(v as i64));
                i += 1;
            }
            "SInt64" | "UInt64" => {
                let v = rd.i64()?;
                if n.meta_flag & META_ALIGN != 0 {
                    rd.align4();
                }
                out.insert(n.field_name.clone(), TyVal::Int(v));
                i += 1;
            }
            "bool" => {
                let v = rd.u8()? != 0;
                if n.meta_flag & META_ALIGN != 0 {
                    rd.align4();
                }
                out.insert(n.field_name.clone(), TyVal::Bool(v));
                i += 1;
            }
            "float" => {
                let v = rd.f32()?;
                if n.meta_flag & META_ALIGN != 0 {
                    rd.align4();
                }
                out.insert(n.field_name.clone(), TyVal::Flt(v));
                i += 1;
            }
            "string" => {
                let len = rd.u32()? as usize;
                let s = rd.take(len)?;
                if n.meta_flag & META_ALIGN != 0 {
                    rd.align4();
                }
                if nodes.get(i + 1).map(|a| a.meta_flag & META_ALIGN != 0).unwrap_or(false) {
                    rd.align4();
                }
                out.insert(
                    n.field_name.clone(),
                    TyVal::Str(String::from_utf8_lossy(s).into_owned()),
                );
                i += 4; // string + Array + size + data（size/data 是描述节点，不占字节）
            }
            "vector" | "staticvector" | "TypelessData" => {
                let count = rd.u32()? as usize;
                // 元素类型节点 = i+3（i 容器, i+1 Array, i+2 size, i+3 data）
                let elem = nodes.get(i + 3).map(|n| n.type_name.as_str()).unwrap_or("");
                let val = if n.type_name == "TypelessData" || matches!(elem, "char" | "UInt8" | "SInt8" | "byte") {
                    let b = rd.take(count)?;
                    TyVal::Bytes(b.to_vec())
                } else if nodes.get(i + 3).map(is_scalar).unwrap_or(false) {
                    let mut arr = Vec::with_capacity(count);
                    for _ in 0..count {
                        arr.push(read_scalar(rd, nodes[i + 3].type_name.as_str())?);
                    }
                    TyVal::Arr(arr)
                } else {
                    // 元素为复合类型：循环解析 count 次
                    let elem_level = nodes[i + 3].level;
                    let mut arr = Vec::with_capacity(count);
                    for _ in 0..count {
                        let mut sub = BTreeMap::new();
                        drive(rd, nodes, i + 3, elem_level, &mut sub)?;
                        arr.push(TyVal::Obj(sub));
                    }
                    TyVal::Arr(arr)
                };
                let array_align = nodes
                    .get(i + 1)
                    .map(|a| a.meta_flag & META_ALIGN != 0)
                    .unwrap_or(false);
                if (n.type_name == "TypelessData" && n.meta_flag & META_ALIGN != 0)
                    || (n.type_name != "TypelessData" && array_align)
                {
                    rd.align4();
                }
                out.insert(n.field_name.clone(), val);
                // string/vector 带 Array 包装（4 节点）；TypelessData 无 Array（3 节点）
                i += if n.type_name == "TypelessData" { 3 } else { 4 };
            }
            "Array" => {
                i += 1; // 已由容器消费
            }
            other => {
                // 复合类型：递归子字段（自身不占字节）
                let mut sub = BTreeMap::new();
                i = drive(rd, nodes, i + 1, n.level + 1, &mut sub)?;
                out.insert(n.field_name.clone(), TyVal::Obj(sub));
                let _ = other;
            }
        }
    }
    Ok(i)
}

fn is_scalar(n: &TtNode) -> bool {
    matches!(
        n.type_name.as_str(),
        "SInt32" | "int" | "signed int" | "UInt32" | "unsigned int" | "SInt64" | "UInt64"
            | "bool" | "float" | "FileSize"
    )
}

fn read_scalar(rd: &mut Le, ty: &str) -> Result<TyVal, String> {
    Ok(match ty {
        "SInt32" | "int" | "signed int" => TyVal::Int(rd.i32()? as i64),
        "UInt32" | "unsigned int" | "FileSize" => TyVal::Int(rd.u32()? as i64),
        "SInt64" | "UInt64" => TyVal::Int(rd.i64()?),
        "bool" => TyVal::Bool(rd.u8()? != 0),
        "float" => TyVal::Flt(rd.f32()?),
        other => return Err(format!("unsupported scalar {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOEL_TEX: &str = "D:/gal/aic-winlator/game-clean/AliceInCradle/AliceInCradle_Data/StreamingAssets/PxlNoel/noel.pxls.bytes.texture_0.dat";

    #[test]
    fn parses_noel_texture_bundle() {
        let d = match std::fs::read(NOEL_TEX) {
            Ok(d) => d,
            Err(_) => return, // 真值文件不在时跳过（CI 环境）
        };
        let (data, nodes) = crate::unityfs::inflate_bundle_ex(&d).unwrap();
        for (i, b) in data.iter().take(64).enumerate() {
            eprint!("{b:02x} ");
            if i % 16 == 15 {
                eprintln!();
            }
        }
        eprintln!();
        let sf = SerializedFile::parse(&data).unwrap();
        eprintln!("unity_version={:?} platform={} objects={}", sf.unity_version, sf.target_platform, sf.objects.len());
        for o in &sf.objects {
            eprintln!("obj path_id={} class={} size={}", o.path_id, o.class_name, o.byte_size);
        }
        let tex = sf.find_by_class("Texture2D").expect("Texture2D not found");
        let f = sf.read_object(tex, &data).unwrap();
        assert_eq!(f.get("m_Width").unwrap().as_int(), Some(4096));
        assert_eq!(f.get("m_Height").unwrap().as_int(), Some(4096));
        assert_eq!(f.get("m_TextureFormat").unwrap().as_int(), Some(12));
        let sd = f.get("m_StreamData").unwrap().as_obj().unwrap();
        assert_eq!(sd.get("size").unwrap().as_int(), Some(16777216));
        assert!(sd.get("path").unwrap().as_str().unwrap().starts_with("archive:/"));
        // resS 节点存在
        assert!(nodes.iter().any(|n| n.path.ends_with(".resS")));
    }
}
