import assert from "node:assert/strict";
import test from "node:test";
import { argumentValue, cliPreview, createDrafts } from "../src/arguments.ts";

test("maps component values to the argument shape accepted by Rust", () => {
  const definitions = [
    { name: "enabled", kind: {type: "flag"}, default: false },
    { name: "max-width", kind: {type: "number"}, default: 640 },
    { name: "source", kind: {type: "path", must_exist: true} },
    { name: "values", kind: {type: "list", item: {type: "number"}}, default: [0, -3] },
  ];
  const values = createDrafts(definitions, "?enabled=&max_width=1280&source=%2Ftmp%2Ffile+with+spaces");
  const data = Object.fromEntries(definitions.map(arg => [arg.name, argumentValue(arg.kind, values[arg.name])]));
  assert.deepEqual(data, {enabled: true, "max-width": 1280, source: "/tmp/file with spaces", values: [0, -3]});
  assert.deepEqual(argumentValue(definitions[3].kind, ["", "7", "oops"]), [7, "oops"]);
  assert.equal(argumentValue({type: "text"}, ""), null);
});

test("preserves literal shell characters in the copyable POSIX preview", () => {
  const standalone = {program: "/tools/img press", uses_subcommand: false};
  assert.equal(cliPreview(standalone, "ImgCompress", { input: "a'$(touch x)", quiet: false }),
    "LILYCO_UI=cli '/tools/img press' --input 'a'\\''$(touch x)'");
  const registry = {program: "/tools/fmt", uses_subcommand: true};
  assert.equal(cliPreview(registry, "inspect", { offset: -3, names: ["--prefix", "two words"] }),
    "LILYCO_UI=cli /tools/fmt inspect --offset=-3 --names=--prefix --names 'two words'");
});
