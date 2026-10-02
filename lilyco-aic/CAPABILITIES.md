# laic — 能力契约（lilyco-aic）

> 由 `laic --schema` 的逐字输出同步（`capabilities.json` 是机器契约，本文件是人读渲染版）。
> 安全分级：**T0** 只读自动放行。

## 命令（5 条，全 T0 只读）

## `laic find-pose`

Search ALL PixelLiner .pxls character tables under `root` (file or directory, scanned recursively) for poses whose title matches `name` (glob, case-insensitive) and report which files contain them — e.g. find which enemy table has a `gun` pose that the player table lacks. Returns { root, files_searched, files_parsed, count, matches: [{ file, poses: [{ title, dirs: [{ dir, frames }], frames_total, aliases }] }] } sorted by file path, plus per-file parse `errors`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| root | path | 是 | — | A .pxls file, or a directory scanned recursively for pxls tables |
| name | text | 否 |  | Pose title glob, e.g. 'gun', '*stand*' |
| limit | number | 否 | 0 | Cap the number of files processed (0 = unlimited) |

## `laic tex`

List Texture2D assets inside all `*.texture_0.dat` UnityFS bundles under `root` (file or directory, scanned recursively), or decode them to PNG files with `--out`. Each texture reports name, size, format (4=RGBA32, 12=DXT1/BC1, 13=DXT5/BC3), storage (inline / .resS stream) and bundle path. PNG export decodes RGBA32 directly and BC1/BC3 via texture2ddecoder, writing `<name>.png` into `--out`. Read-only (safety T0).

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
