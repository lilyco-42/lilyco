# laic — 能力契约（lilyco-aic）

> 由 `laic --schema` 的逐字输出同步（`capabilities.json` 是机器契约，本文件是人读渲染版）。
> 安全分级：**T0** 只读自动放行；**T1**（`repack`）需人工确认，MCP 自动化面默认拒绝。

## 命令（8 条：7 条 T0 只读 + `repack` T1）

## `laic find-pose`

Search ALL PixelLiner .pxls character tables under `root` (file or directory, scanned recursively) for poses whose title matches `name` (glob, case-insensitive) and report which files contain them — e.g. find which enemy table has a `gun` pose that the player table lacks. Returns { root, files_searched, files_parsed, count, matches: [{ file, poses: [{ title, dirs: [{ dir, frames }], frames_total, aliases }] }] } sorted by file path, plus per-file parse `errors`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| name | text | 否 |  | Pose title glob, e.g. 'gun', '*stand*' |
| limit | number | 否 | 0 | Cap the number of files processed (0 = unlimited) |

## `laic tex`

List Texture2D assets inside all `*.texture_0.dat` UnityFS bundles under `root` (file or directory, scanned recursively), or decode them to PNG files with `--out`. Each texture reports name, size, format (Unity `TextureFormat` numbering: 1=Alpha8, 2=ARGB4444, 3=RGB24, 4=RGBA32, 5=ARGB32, 7=RGB565, 9=R16, 10=DXT1/BC1, 12=DXT5/BC3, 13=RGBA4444, 14=BGRA32, 25=BC7, 26=BC4, 27=BC5, 28=DXT1Crunched, 29=DXT5Crunched, 48-51=ASTC 4x4/5x5/6x6/8x8), storage (inline / .resS stream) and bundle path. PNG export decodes every listed format (block formats via texture2ddecoder, the rest with built-in unpackers) and flips the row order (Unity stores Texture2D pixels bottom-up), writing `<name>.png` into `--out`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .texture_0.dat file, or a directory scanned recursively for texture bundles |
| name | text | 否 |  | Texture2D name glob filter, e.g. 'noel*' |
| out | text | 否 |  | Directory to write decoded PNG files into (optional) |
| limit | number | 否 | 0 | Cap the number of bundles processed (0 = unlimited) |

## `laic frame`

Dump every frame and layer transform of poses matching `pose` (glob on title, case-insensitive) from PixelLiner .pxls tables. This is the character-structure data a mod needs: per layer { name, kind, group, alpha, x, y (units), zmx, zmy, rot_r (radians), blend_variable, img (image key EDI<hex>_<id2>), img_size (px, when the image is found in an atlas) }. `root` is a pxls file or a directory; `index` restricts to one frame index per direction sequence. Returns { root, matches: [{ file, pose, dirs: [{ dir, frames: [{ index, name, crf60, layers: [...] }] }] }] }. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| pose | text | 是 | — | Pose title glob, e.g. 'gun', 'gun2stand' |
| index | number | 否 | — | Only frame N within each direction sequence (0-based; omit for all) |

## `laic poses`

List poses defined in PixelLiner .pxls character tables (raw files or UnityFS-wrapped .pxls.dat, e.g. Alice in Cradle StreamingAssets). `root` may be a single file or a directory scanned recursively for *.pxls.dat / *.pxls / *.pxls.bytes. Returns { root, files_searched, files_parsed, poses: [{ file, title, width, height, auto_flip, aliases, dirs: [{ dir, frames, loop_to }], frames_total }] } sorted by file path; `pose` filters by pose title with a glob (`*`/`?`, case-insensitive); `full` additionally embeds every frame's layer table (name, type, group, alpha, x, y, zmx, zmy, rot_r, img, img_size) — this is the full character-structure dump. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| pose | text | 否 | — | Glob filter on pose TITLE, e.g. 'gun*' (omit for all poses) |
| full | flag | 否 | — | Also embed every frame's layer transform table (name/type/x/y/zmx/zmy/rot_r/img) |
| limit | number | 否 | 0 | Cap the number of files processed (0 = unlimited) |

## `laic mpcc`

Parse the header of `*.mpcc.bytes` MobPCCContainer files (mobpcc/ skin & recolor containers) under `root` (file or directory, scanned recursively): reports container `name`, character `chr_name`, palette presence flag and remaining payload size. Deep ACC palette parsing (per-parts recolor table) is planned — this command is the inventory step. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .mpcc.bytes file, or a directory scanned recursively for MPCC containers |
| name | text | 否 |  | Container name glob filter, e.g. 'NOEL*' |
| limit | number | 否 | 0 | Cap the number of files processed (0 = unlimited) |

## `laic sprites`

Export every sprite of PixelLiner .pxls character tables as an individual PNG by cropping the packed atlas with the `%PACK_SECTION%` UV table (the data a sprite/mod pipeline needs). For each table under `root` (file or directory, scanned recursively) it pairs every atlas entry with its pixels the way the game does: atlas index i takes the Texture2D whose asset name ends with `_i` inside `<stem>.pxls.bytes.texture_0.dat` (the game packs both `texture_0` and the parts `texture_1` in that one bundle — matched by name, never by object order), falling back to a separate `<stem>.pxls.bytes.texture_<i>` file or an embedded PNG. Decoding covers DXT5/BC3, DXT1/BC1, BC7, BC4/BC5, DXT1/5Crunched, RGBA32, RGB24, RGB565, ARGB4444, RGBA4444, BGRA32 and ASTC. Output is `<out>/<table>/<layer-name>.png`. Sprites are named after the POSE LAYER that uses the image (falling back to the raw EDI<hex>_<id2> key when no layer references it); sprites taken from a secondary atlas get an `.a<i>` suffix. A PARTS atlas whose UV count is 0 inherits the previous atlas's UV table (same rects, different texture — the torn/clothing-overlay variant), exactly like `PxlsImgAtlas.readFromBytes` does. `margin` from the atlas entry is honoured (`x+margin, y+margin, w-2m, h-2m`) and the crop uses PNG top-left origin (no Y flip). Flags: `atlas` also dumps the whole decoded atlas, `embedded` also writes PNGs embedded in the `%IMGS_SECTION%`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| out | path | 是 | — | Directory to write the cropped sprite PNG files into |
| pose | text | 否 |  | Only export sprites used by poses whose title matches this glob |
| atlas | flag | 否 | — | Also write the whole decoded atlas as \<table\>.atlas_\<i\>.png |
| embedded | flag | 否 | — | Also write PNGs embedded in the %IMGS_SECTION% |
| limit | number | 否 | 0 | Cap the number of tables processed (0 = unlimited) |

## `laic render`

Composite PixelLiner pose frames into PNG images (and optionally an animated GIF per direction and/or a sprite sheet per pose) — i.e. actually DRAW the character instead of dumping numbers. Semantics are taken from the game's own mesh builder (`PxlMeshDrawer.makeMesh` -> `RotaGraph`): per layer the sprite is placed with its centre at (layer.x, layer.y) in a `pose.width x pose.height` canvas, scaled by (zmx, zmy), rotated by -rot_r, blended with alpha = layer.alpha/100, sampled nearest-neighbour (Point filter, required for pixel art). `root` is a pxls file or a directory scanned recursively; `pose` is a title glob (default `*`); `dir`/`frame` narrow to one direction / frame index. Writes `<out>/<table>.<pose>.d<dir>.f<frame>.png`; `anim` writes `<table>.<pose>.d<dir>.gif`; `sheet` writes `<table>.<pose>.sheet.png` (rows = directions, columns = frames). `scale` is an integer upscale. The atlas is paired automatically (`<pxls>.pxls.bytes.texture_<i>.dat`, or bare `.texture_0.png`, or an embedded PNG); `texture` overrides the file for atlas 0. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| out | path | 是 | — | Directory to write the rendered PNG/GIF/sheet files into |
| pose | text | 否 | * | Pose title glob filter (case-insensitive) |
| dir | number | 否 | 99 | Only render direction N (0..=7; omit for all) |
| frame | number | 否 | 99 | Only render frame index N within each direction (omit for all) |
| scale | number | 否 | 1 | Integer upscale factor for the output image |
| anim | flag | 否 | — | Also write one animated GIF per direction |
| sheet | flag | 否 | — | Also write a sprite sheet per pose (rows = directions, columns = frames) |
| texture | text | 否 |  | Explicit texture file for atlas 0 (overrides auto pairing) |
| limit | number | 否 | 0 | Cap the number of tables processed (0 = unlimited) |

## `laic repack`（T1，需人工确认）

Write a modified PixelLiner .pxls table back into its container — the save half of a sprite/skin pipeline (read-only tools can only look, this one can commit). Edits are applied to the parsed model and then re-serialized, byte-exactly, back into EITHER a bare .pxls file OR the game's UnityFS bundle (`.pxls.dat`): for the bundle path it locates the TextAsset inside the v22 SerializedFile, splices the new body in place and repairs the TypelessData length prefix, the SerializedFile header `file_size`, the object table (byte_size / trailing byte_start) and the UnityFS blocks info + node table. Edits: `rename` (OLD=NEW, repeatable or `;`-separated, renames pose titles), `set-alpha` (LAYER=0..100, sets a layer's alpha in every matching pose). `pose` is a title glob limiting which poses may be edited. `compress` picks how the rewritten bundle is packed: `none`, `lz4`, or `original` (keep the source algorithm; Unity LZMA has no encoder here and degrades to none). DRY RUN BY DEFAULT — pass `apply: true` to actually write (safety tier T1: the automated/MCP surface denies it, so a human must confirm). Every written file is re-read and re-parsed; the round-trip must be byte-identical or the command fails. Writes `<out>/<file-name>`.

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| out | path | 是 | — | Directory to write the repacked files into |
| pose | text | 否 | * | Only poses whose title matches this glob may be edited |
| rename | list | 否 | [] | Rename pose titles, `OLD=NEW` (repeatable, or `;`-separated) |
| set-alpha | list | 否 | [] | Set a layer's alpha, `LAYER=0..100` (repeatable, or `;`-separated) |
| compress | text | 否 | original | Compression for the rewritten bundle: none \| lz4 \| original |
| apply | flag | 否 | — | Actually write the files. Without this flag the command is a dry run |
| overwrite | flag | 否 | — | Allow overwriting an existing output file (default: skip it) |
| limit | number | 否 | 0 | Cap the number of tables processed (0 = unlimited) |
