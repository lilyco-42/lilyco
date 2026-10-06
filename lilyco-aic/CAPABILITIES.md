# laic 能力表（capabilities）

> 由 `lilyco doc` 生成。机器契约是同目录 `capabilities.json`（app `--schema` 的逐字输出），本文件是人读的渲染版。
>
> 安全分级：**T0** 只读自动放行 · **T1** 需人工确认 · **T2** 需能力令牌 · **T3** 禁止自动化执行。

共 10 条命令。

## `tex` — T0 只读

List Texture2D assets inside all `*.texture_0.dat` UnityFS bundles under `root` (file or directory, scanned recursively), or decode them to PNG files with `--out`. Each texture reports name, size, format (Unity `TextureFormat` numbering: 1=Alpha8, 2=ARGB4444, 3=RGB24, 4=RGBA32, 5=ARGB32, 7=RGB565, 9=R16, 10=DXT1/BC1, 12=DXT5/BC3, 13=RGBA4444, 14=BGRA32, 25=BC7, 26=BC4, 27=BC5, 28=DXT1Crunched, 29=DXT5Crunched, 48-51=ASTC 4x4/5x5/6x6/8x8), storage (inline / .resS stream) and bundle path. PNG export decodes every listed format (block formats via texture2ddecoder, the rest with built-in unpackers) and flips the row order (Unity stores Texture2D pixels bottom-up), writing `<name>.png` into `--out`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .texture_0.dat file, or a directory scanned recursively for texture bundles |
| `name` | text |  | "" | Texture2D name glob filter, e.g. 'noel*' |
| `out` | text |  | "" | Directory to write decoded PNG files into (optional) |
| `limit` | number |  | 0 | Cap the number of bundles processed (0 = unlimited) |

## `render` — T0 只读

Composite PixelLiner pose frames into PNG images (and optionally an animated GIF per direction and/or a sprite sheet per pose) — i.e. actually DRAW the character instead of dumping numbers. Semantics are taken from the game's own mesh builder (`PxlMeshDrawer.makeMesh` -> `RotaGraph`): per layer the sprite is placed with its centre at (layer.x, layer.y) in a `pose.width x pose.height` canvas, scaled by (zmx, zmy), rotated by -rot_r, blended with alpha = layer.alpha/100, sampled nearest-neighbour (Point filter, required for pixel art). `root` is a pxls file or a directory scanned recursively; `pose` is a title glob (default `*`); `dir`/`frame` narrow to one direction / frame index. Writes `<out>/<table>.<pose>.d<dir>.f<frame>.png`; `anim` writes `<table>.<pose>.d<dir>.gif`; `sheet` writes `<table>.<pose>.sheet.png` (rows = directions, columns = frames). `scale` is an integer upscale. The atlas is paired automatically (`<pxls>.pxls.bytes.texture_<i>.dat`, or bare `.texture_0.png`, or an embedded PNG); `texture` overrides the file for atlas 0. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `out` | path | ✓ | — | Directory to write the rendered PNG/GIF/sheet files into |
| `pose` | text |  | "*" | Pose title glob filter (case-insensitive) |
| `dir` | number |  | 99 | Only render direction N (0..=7; omit for all) |
| `frame` | number |  | 99 | Only render frame index N within each direction (omit for all) |
| `scale` | number |  | 1 | Integer upscale factor for the output image |
| `anim` | flag |  | false | Also write one animated GIF per direction |
| `sheet` | flag |  | false | Also write a sprite sheet per pose (rows = directions, columns = frames) |
| `texture` | text |  | "" | Explicit texture file for atlas 0 (overrides auto pairing) |
| `parts` | flag |  | false | Composite from the PARTS atlas (texture_1) instead of the primary one — the torn/clothing-overlay variant. Tables without a second atlas are unaffected |
| `limit` | number |  | 0 | Cap the number of tables processed (0 = unlimited) |

## `sprites` — T0 只读

Export every sprite of PixelLiner .pxls character tables as an individual PNG by cropping the packed atlas with the `%PACK_SECTION%` UV table (the data a sprite/mod pipeline needs). For each table under `root` (file or directory, scanned recursively) it pairs every atlas entry with its pixels the way the game does: atlas index i takes the Texture2D whose asset name ends with `_i` inside `<stem>.pxls.bytes.texture_0.dat` (the game packs both `texture_0` and the parts `texture_1` in that one bundle — matched by name, never by object order), falling back to a separate `<stem>.pxls.bytes.texture_<i>` file or an embedded PNG. Decoding covers DXT5/BC3, DXT1/BC1, BC7, BC4/BC5, DXT1/5Crunched, RGBA32, RGB24, RGB565, ARGB4444, RGBA4444, BGRA32 and ASTC. Output is `<out>/<table>/<layer-name>.png`. Sprites are named after the POSE LAYER that uses the image (falling back to the raw EDI<hex>_<id2> key when no layer references it); sprites taken from a secondary atlas get an `.a<i>` suffix. A PARTS atlas whose UV count is 0 inherits the previous atlas's UV table (same rects, different texture — the torn/clothing-overlay variant), exactly like `PxlsImgAtlas.readFromBytes` does. `margin` from the atlas entry is honoured (`x+margin, y+margin, w-2m, h-2m`) and the crop uses PNG top-left origin (no Y flip). Flags: `atlas` also dumps the whole decoded atlas, `embedded` also writes PNGs embedded in the `%IMGS_SECTION%`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `out` | path | ✓ | — | Directory to write the cropped sprite PNG files into |
| `pose` | text |  | "" | Only export sprites used by poses whose title matches this glob |
| `atlas` | flag |  | false | Also write the whole decoded atlas as <table>.atlas_<i>.png |
| `embedded` | flag |  | false | Also write PNGs embedded in the %IMGS_SECTION% |
| `limit` | number |  | 0 | Cap the number of tables processed (0 = unlimited) |

## `mpcc` — T0 只读

Parse `*.mpcc.bytes` MobPCCContainer files (mobpcc/ portrait skin & recolor containers) under `root` (file or directory, scanned recursively). This is the data behind Alice in Cradle's outfit/colour customisation: the container names a character and holds a per-PARTS list of colour operations that the game applies on the GPU. Reported per container: `name`, `chr_name`, and every parts entry with its ops — `hsv` (h raw + degrees, s/v raw + percent, flags) and `tone_curve` (channel count and per-channel point counts; pass `full` to also emit the 0..255 key-point lists). Every file is re-serialised and compared byte-for-byte against the input (`roundtrip_ok`), so `true` proves the parse is complete — including the two places where the game reads-and-discards bytes (op type 0, and parts entries whose op count is 0); `dropped` reports how many were discarded. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .mpcc.bytes file, or a directory scanned recursively for MPCC containers |
| `name` | text |  | "" | Container name glob filter, e.g. 'NOEL*' |
| `full` | flag |  | false | Also emit the tone-curve key-point lists (each channel can hold up to 255 points) |
| `limit` | number |  | 0 | Cap the number of files processed (0 = unlimited) |

## `find-pose` — T0 只读

Search ALL PixelLiner .pxls character tables under `root` (file or directory, scanned recursively) for poses whose title matches `name` (glob, case-insensitive) and report which files contain them — e.g. find which enemy table has a `gun` pose that the player table lacks. Returns { root, files_searched, files_parsed, count, matches: [{ file, poses: [{ title, dirs: [{ dir, frames }], frames_total, aliases }] }] } sorted by file path, plus per-file parse `errors`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `name` | text |  | "" | Pose title glob, e.g. 'gun', '*stand*' |
| `limit` | number |  | 0 | Cap the number of files processed (0 = unlimited) |

## `frame` — T0 只读

Dump every frame and layer transform of poses matching `pose` (glob on title, case-insensitive) from PixelLiner .pxls tables. This is the character-structure data a mod needs: per layer { name, kind, group, alpha, x, y (units), zmx, zmy, rot_r (radians), blend_variable, img (image key EDI<hex>_<id2>), img_size (px, when the image is found in an atlas) }. `root` is a pxls file or a directory; `index` restricts to one frame index per direction sequence. Returns { root, matches: [{ file, pose, dirs: [{ dir, frames: [{ index, name, crf60, layers: [...] }] }] }] }. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `pose` | text | ✓ | — | Pose title glob, e.g. 'gun', 'gun2stand' |
| `index` | number |  | — | Only frame N within each direction sequence (0-based; omit for all) |

## `repack` — T1 需确认

Write a modified PixelLiner .pxls table back into its container — the save half of a sprite/skin pipeline (read-only tools can only look, this one can commit). Edits are applied to the parsed model and then re-serialized, byte-exactly, back into EITHER a bare .pxls file OR the game's UnityFS bundle (`.pxls.dat`): for the bundle path it locates the TextAsset inside the v22 SerializedFile, splices the new body in place and repairs the TypelessData length prefix, the SerializedFile header `file_size`, the object table (byte_size / trailing byte_start) and the UnityFS blocks info + node table. Edits: `rename` (OLD=NEW, repeatable or `;`-separated, renames pose titles), `set-alpha` (LAYER=0..100, sets a layer's alpha in every matching pose). `pose` is a title glob limiting which poses may be edited. `compress` picks how the rewritten bundle is packed: `none`, `lz4`, or `original` (keep the source algorithm; Unity LZMA has no encoder here and degrades to none). DRY RUN BY DEFAULT — pass `apply: true` to actually write (safety tier T1: the automated/MCP surface denies it, so a human must confirm). Every written file is re-read and re-parsed; the round-trip must be byte-identical or the command fails. Writes `<out>/<file-name>`.

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `out` | path | ✓ | — | Directory to write the repacked files into |
| `pose` | text |  | "*" | Only poses whose title matches this glob may be edited |
| `rename` | list<text> |  | [] | Rename pose titles, `OLD=NEW` (repeatable, or `;`-separated) |
| `set-alpha` | list<text> |  | [] | Set a layer's alpha, `LAYER=0..100` (repeatable, or `;`-separated) |
| `compress` | text |  | "original" | Compression for the rewritten bundle: none \| lz4 \| original |
| `apply` | flag |  | — | Actually write the files. Without this flag the command is a dry run |
| `overwrite` | flag |  | — | Allow overwriting an existing output file (default: skip it) |
| `limit` | number |  | 0 | Cap the number of tables processed (0 = unlimited) |

## `poses` — T0 只读

List poses defined in PixelLiner .pxls character tables (raw files or UnityFS-wrapped .pxls.dat, e.g. Alice in Cradle StreamingAssets). `root` may be a single file or a directory scanned recursively for *.pxls.dat / *.pxls / *.pxls.bytes. Returns { root, files_searched, files_parsed, poses: [{ file, title, width, height, auto_flip, aliases, dirs: [{ dir, frames, loop_to }], frames_total }] } sorted by file path; `pose` filters by pose title with a glob (`*`/`?`, case-insensitive); `full` additionally embeds every frame's layer table (name, type, group, alpha, x, y, zmx, zmy, rot_r, img, img_size) — this is the full character-structure dump. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `pose` | text |  | — | Glob filter on pose TITLE, e.g. 'gun*' (omit for all poses) |
| `full` | flag |  | — | Also embed every frame's layer transform table (name/type/x/y/zmx/zmy/rot_r/img) |
| `limit` | number |  | 0 | Cap the number of files processed (0 = unlimited) |

## `animate` — T0 只读

Drive a PixelLiner pose sequence as a steppable animation state machine — the runtime half that `render` does not expose. Mirrors pixelliner4j's FrameAnimator exactly: each frame lasts `crf60` ticks of 1/60 s, `step` accumulates ticks and advances when the budget is spent, `stepFrame` jumps a whole frame, `changeFrame(name)` seeks by frame name (case-insensitive), and running past the end wraps to `loop_to` and increments `looped_count`. Answers the modding question "how many ticks is one loop of this animation, and which frame does it return to". Select the sequence with `pose` (title glob) + `dir` (0..=7); seek with `frame` (name) or `index`; then advance with `ticks` (tick-accurate) and/or `frames` (whole frames). `out` renders the frame the playhead landed on. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `pose` | text |  | "*" | Pose title glob filter (case-insensitive) |
| `dir` | number |  | 99 | Only direction N (0..=7; omit for all) |
| `index` | number |  | 99 | Seek to frame index N before advancing (0-based) |
| `frame` | text |  | "" | Seek to the frame named N (case-insensitive; takes precedence over index) |
| `ticks` | number |  | 0 | Advance N ticks of 1/60 s (accumulates against each frame's crf60) |
| `frames` | number |  | 0 | Additionally jump N whole frames (ignores crf60) |
| `out` | text |  | "" | Directory to render the frame the playhead landed on into (empty = no render) |
| `scale` | number |  | 1 | Integer upscale factor when rendering |
| `texture` | text |  | "" | Explicit texture file for atlas 0 (overrides auto pairing) |
| `parts` | flag |  | false | Render the landed frame from the PARTS atlas (texture_1) instead of the primary one |
| `limit` | number |  | 0 | Cap the number of tables processed (0 = unlimited) |

## `pack` — T1 需确认

Re-pack a PixelLiner atlas: crop every sprite out of the current atlas (optionally substituting edited PNGs from `replace`), lay them out again (default `shelf` = rows filled height-descending, ~92% occupancy; `--packer guillotine` reproduces pixelliner4j's naive node-tree packer, which measured only 20% on noel and needs twice the atlas side), trim the canvas to what is actually used, and write a self-contained `.pxl` with the new atlas embedded as PNG. This is the missing half of the edit loop — `sprites` cuts, you edit, `pack` puts it back. Unlike pixelliner4j's equivalent it KEEPS the IMGS/IMGV/PTCL sections, keeps dual atlases (the PARTS atlas reuses the same rects and keeps its empty UV list so the inheritance rule still fires), keeps each UV's w/h so rendered sizes never drift, and reserves `margin` pixels around every sprite so neighbours cannot bleed. Layer keys are untouched, so every pose/frame keeps working unchanged. `replace` is a directory of PNGs named exactly as `sprites` writes them: `<layer-name>.png` for the FIRST sprite with that layer name, `<layer-name>.<img-key>.png` when the name is shared (617 keys are called `Layer` in noel, so a bare `Layer.png` matches only one of them), or `<img-key>.png`; non-primary atlases add `.a<atlas>`. Files that match nothing are reported in `unmatched_replacements` — never dropped silently. If a replacement changes a sprite's SIZE, the PARTS atlas that shares the UV table follows it (reported as carried/fitted) so both stay geometrically identical. Writes `<out>/<table>.pxl`; `atlas` also dumps `<out>/<table>.atlas_<i>.png`. DRY RUN BY DEFAULT — pass `apply: true` to write (safety tier T1: the automated/MCP surface denies it). Every written file is re-read, re-parsed and its sprites compared pixel-by-pixel against what went in. Suggest running `sprites` first so the replacement PNGs carry the exact export names.

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `out` | path | ✓ | — | Directory to write the .pxl files into |
| `replace` | text |  | "" | Directory of replacement PNGs (named as `sprites` writes them) |
| `pose` | text |  | "" | Pose title glob, only to reproduce the exact file names `sprites --pose` produced |
| `size` | number |  | 0 | Starting atlas side in px (0 = derive from the original atlas) |
| `packer` | text |  | "shelf" | Packing algorithm: shelf (default) \| guillotine (pixelliner4j's, for comparison) |
| `atlas` | flag |  | false | Also dump the re-packed atlas PNG for inspection |
| `apply` | flag |  | — | Actually write the files. Without this flag the command is a dry run |
| `overwrite` | flag |  | — | Allow overwriting an existing output file (default: skip it) |
| `limit` | number |  | 0 | Cap the number of tables processed (0 = unlimited) |

