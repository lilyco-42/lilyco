# lpxls 能力表（capabilities）

> 由 `lilyco doc` 生成。机器契约是同目录 `capabilities.json`（app `--schema` 的逐字输出），本文件是人读的渲染版。
>
> 安全分级：**T0** 只读自动放行 · **T1** 需人工确认 · **T2** 需能力令牌 · **T3** 禁止自动化执行。

共 3 条命令。

## `frame` — T0 只读

Dump every frame and layer transform of poses matching `pose` (glob on title, case-insensitive) from PixelLiner .pxls tables. This is the character-structure data a mod needs: per layer { name, kind, group, alpha, x, y (units), zmx, zmy, rot_r (radians), blend_variable, img (image key EDI<hex>_<id2>), img_size (px, when the image is found in an atlas) }. `root` is a pxls file or a directory; `index` restricts to one frame index per direction sequence. Returns { root, matches: [{ file, pose, dirs: [{ dir, frames: [{ index, name, crf60, layers: [...] }] }] }] }. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `pose` | text | ✓ | — | Pose title glob, e.g. 'gun', 'gun2stand' |
| `index` | number |  | — | Only frame N within each direction sequence (0-based; omit for all) |

## `poses` — T0 只读

List poses defined in PixelLiner .pxls character tables (raw files or UnityFS-wrapped .pxls.dat, e.g. Alice in Cradle StreamingAssets). `root` may be a single file or a directory scanned recursively for *.pxls.dat / *.pxls / *.pxls.bytes. Returns { root, files_searched, files_parsed, poses: [{ file, title, width, height, auto_flip, aliases, dirs: [{ dir, frames, loop_to }], frames_total }] } sorted by file path; `pose` filters by pose title with a glob (`*`/`?`, case-insensitive); `full` additionally embeds every frame's layer table (name, type, group, alpha, x, y, zmx, zmy, rot_r, img, img_size) — this is the full character-structure dump. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `pose` | text |  | — | Glob filter on pose TITLE, e.g. 'gun*' (omit for all poses) |
| `full` | flag |  | — | Also embed every frame's layer transform table (name/type/x/y/zmx/zmy/rot_r/img) |
| `limit` | number |  | 0 | Cap the number of files processed (0 = unlimited) |

## `find-pose` — T0 只读

Search ALL PixelLiner .pxls character tables under `root` (file or directory, scanned recursively) for poses whose title matches `name` (glob, case-insensitive) and report which files contain them — e.g. find which enemy table has a `gun` pose that the player table lacks. Returns { root, files_searched, files_parsed, count, matches: [{ file, poses: [{ title, dirs: [{ dir, frames }], frames_total, aliases }] }] } sorted by file path, plus per-file parse `errors`. Read-only (safety T0).

| 参数 | 类型 | 必填 | 缺省 | 说明 |
|---|---|---|---|---|
| `root` | path（须存在） | ✓ | — | A .pxls file, or a directory scanned recursively for pxls tables |
| `name` | text |  | "" | Pose title glob, e.g. 'gun', '*stand*' |
| `limit` | number |  | 0 | Cap the number of files processed (0 = unlimited) |

