import assert from "node:assert/strict";
import test from "node:test";
import { resolveLocale } from "../src/locale.ts";

test("language uses the first supported browser preference", () => {
  assert.equal(resolveLocale(["fr-CA", "zh-Hant-TW", "en-GB"]), "zh-CN");
  assert.equal(resolveLocale(["EN-GB", "zh-CN"]), "en");
  assert.equal(resolveLocale(["ja-JP", "de-DE"]), "en");
  assert.equal(resolveLocale([]), "en");
});
