## Type -> Widget Mapping

| Rust Type | CLI | TUI | Web |
|-----------|-----|-----|-----|
| `bool` | `--flag` | `[x]` Space toggle | `<input type=checkbox>` |
| `String` | `--name <val>` | text input | `<input type=text>` |
| `u8`/`i32`/`f64`/... | `--count <num>` | ^v +/-1 + digit input | `<input type=number>` |
| Custom enum | `--mode <choice>` | <-> cycle | `<select>` |
| `PathBuf` | `--file <path>` | text input | `<input type=text>` |
| `Vec<T>` | `--tag a --tag b` | Enter/Delete multi-line | dynamic inputs |
| `Option<T>` | optional | optional (not required) | optional |

---

