# Lilyco shadcn/ui adapter

`--gui` serves this React and shadcn/ui adapter at `/`. The static bundle is embedded in the Rust executable and served from the loopback origin. Prior console URLs remain available under `/classic`. Embedded dependencies and upstream licenses are listed at `/ui/notices.txt`.

The theme defaults to System and updates when the browser reports an OS appearance change. Light and Dark are saved as explicit preferences. Both renderers share the same setting on the loopback origin.

Language also defaults to Follow system. Both renderers use the browser's ordered language preferences, recognize any Chinese or English regional variant, and fall back to English when neither is available. An explicit choice is saved on the loopback origin and shared with `/classic`. Changing language keeps the current form and results.

Apps can set `#[app(ui_messages = "../i18n/my-command.toml")]` to embed localized command and argument descriptions. Use `[zh-CN]` or `[en]` for an optional `about` and the corresponding `[zh-CN.args]` or `[en.args]` table for labels keyed by the schema's argument names. A missing translation uses the original schema description.

The command form and live output sit side by side on wide screens and stack with the same keyboard order on smaller screens. Existing-file arguments lead with file selection and drag-and-drop, while leaving the path editable. The run action offers a direct link to output on narrow screens. Results appear above logs so successful runs remain easy to scan.

## Contracts

- `lilyco-core` defines `ArgSchema`, `Component`, `Progress`, `UiCatalog`, and `UiRunRequest`. The optional `ui-schema` feature exports JSON Schema for tools.
- `/ui/catalog` returns visible commands, their arguments and view slots, the active command, and cancellation capability. `/run` uses the existing Rust argument validation. `/progress/{id}` streams progress, logs, views, and the final result.
- The catalog may include the executable and its invocation style. The CLI card generates a POSIX shell command using those fields, with the selected command as a subcommand only in registry mode.
- `src/arguments.ts` converts native browser strings to the declared argument shape. File controls upload browser `File` objects and write the returned path to the argument. Numeric ranges and required fields come from Rust. `src/views.tsx` maps existing semantic view types to React components. An unknown view type renders a diagnostic with its data.
- Semantic output forms retain local input state. Command actions in rendered views select a visible command so a user can fill its arguments and confirm execution. Output pagination displays the page metadata supplied by the command. Action fields for server-side output-widget interactions can be added to the protocol when a command needs them.
- The browser owns component props, visual variants, layout, tokens, focus, and translations. shadcn component source is tracked with `components.json` and `registry-lock.json`. Updating a frontend component uses this adapter and its tests. A change to a command's accepted data goes through the Rust definitions.

## Build

Run these commands from the repository root with Rust, Node.js 22.12+, and pnpm 12.

```sh
pnpm --dir lilyco-gui/web install --frozen-lockfile
pnpm --dir lilyco-gui/web contract
pnpm --dir lilyco-gui/web test
pnpm --dir lilyco-gui/web build
cargo test -p lilyco-core --features ui-schema -p lilyco-gui
cargo run -p lilyco-example --features lilyco/web --bin imgpress -- --gui
```

`pnpm --dir lilyco-gui/web contract:check` verifies the generated TypeScript declarations without changing files. `build` checks registry content hashes, compiles TypeScript and CSS, and regenerates the dependency notices in `assets/shadcn/`. Commit the generated assets with their source changes. Cargo builds can use the committed assets offline.

The MIT licenses for input-otp 1.4.2 and react-remove-scroll-bar 2.3.8 are provided by upstream. Their published packages omit the license files.

- [input-otp source license](https://github.com/guilhermerodz/input-otp/blob/81ccdb48c010d800b24942aa231909f0c971b1ca/LICENSE)
- [react-remove-scroll-bar source license](https://github.com/theKashey/react-remove-scroll-bar/blob/8ca9ba5ea52de03308fe8ced94f7b159a44d28ff/LICENSE)
- [shadcn/ui Registry documentation](https://ui.shadcn.com/docs/registry)
