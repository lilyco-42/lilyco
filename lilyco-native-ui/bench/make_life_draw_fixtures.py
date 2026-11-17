#!/usr/bin/env python3
"""Generate draw-op-scaled Life Web fixtures from the canonical JS_GAME source.

The Life simulation, seed, grid and digest are identical for every repeat count;
only the number of identical fill_rect passes per frame grows.
"""
import ast
import hashlib
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE / 'make_life_fixtures.py'
GEN = HERE / 'fixtures' / '.generated'
V = HERE / 'vendor'
GEN.mkdir(parents=True, exist_ok=True)

tree = ast.parse(SRC.read_text())
CONST = {}
for node in tree.body:
    if (isinstance(node, ast.Assign) and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Name)
            and node.targets[0].id in ('HEAD', 'CSS', 'JS_GAME')
            and isinstance(node.value, ast.Constant)):
        CONST[node.targets[0].id] = node.value.value
assert set(CONST) == {'HEAD', 'CSS', 'JS_GAME'}

REPEATS = [1, 2, 4, 8, 16]
OLD_DRAW = """function drawLife(ctx, life) {
  ctx.fillStyle = '#101820';
  ctx.fillRect(0, 0, WIDTH, HEIGHT);
  ctx.fillStyle = '#4ade80';
  const cells = life.cells;
  for (let y = 0; y < ROWS; y++) {
    for (let x = 0; x < COLS; x++) {
      if (cells[y * COLS + x] === 1) ctx.fillRect(x * CELL, y * CELL, CELL - 1, CELL - 1);
    }
  }
}"""
NEW_DRAW = """function drawLife(ctx, life) {
  ctx.fillStyle = '#101820';
  ctx.fillRect(0, 0, WIDTH, HEIGHT);
  ctx.fillStyle = '#4ade80';
  const cells = life.cells;
  for (let rep = 0; rep < DRAW_REPEAT; rep++) {
    for (let y = 0; y < ROWS; y++) {
      for (let x = 0; x < COLS; x++) {
        if (cells[y * COLS + x] === 1) ctx.fillRect(x * CELL, y * CELL, CELL - 1, CELL - 1);
      }
    }
  }
}"""

def make_js(tag, repeat):
    js = CONST['JS_GAME']
    js, n1 = re.subn(r'const COLS = \d+, ROWS = \d+, CELL = [\d.]+;',
                     'const COLS = 360, ROWS = 240, CELL = 2;', js, count=1)
    js, n2 = re.subn(r'const DIGEST_STEPS = \d+, TICK_US = \d+;',
                     'const DIGEST_STEPS = 60, TICK_US = 16667;\nconst DRAW_REPEAT = ' + str(repeat) + ';', js, count=1)
    js, n3 = re.subn(r"',tick_us=' \+ TICK_US;",
                     "',tick_us=' + TICK_US + ',draw_repeat=' + DRAW_REPEAT;", js, count=1)
    js, n4 = re.subn(r"'Lilyco Life - React/Web'", "'Lilyco Life - " + tag + "'", js, count=1)
    assert (n1, n2, n3, n4) == (1, 1, 1, 1), (tag, n1, n2, n3, n4)
    assert OLD_DRAW in js, tag
    js = js.replace(OLD_DRAW, NEW_DRAW, 1)
    assert 'DRAW_REPEAT' in js and 'draw_repeat=' in js
    return js

def make_html(tag, js):
    title = 'Lilyco Life - ' + tag
    return ('<!DOCTYPE html>\n<html lang="zh-CN">\n<head>\n<meta charset="UTF-8">\n'
            '<meta name="viewport" content="width=device-width, initial-scale=1.0">\n'
            '<title>' + title + '</title>' + CONST['HEAD'] + '\n<style>' + CONST['CSS'] + '</style>\n'
            '</head>\n<body>\n<div id="root"></div>\n'
            '<script type="text/babel" data-presets="lilyco-react">' + js + '</script>\n'
            '</body>\n</html>\n')

def inline_scripts(html):
    assets = iter(['react-18.3.1.js', 'react-dom-18.3.1.js', 'babel-7.28.4.js'])
    def sub(m):
        return '<script>' + (V / next(assets)).read_text() + '</script>'
    out, n = re.subn(r'<script[^>]+src="https://unpkg\.com/[^"]+"[^>]*></script>', sub, html)
    assert n == 3, n
    assert 'unpkg' not in out
    return out

for repeat in REPEATS:
    tag = 'draw-' + str(repeat)
    assert 360 * 2 == 720 and 240 * 2 == 480
    html = make_html(tag, make_js(tag, repeat))
    assert html.count('Babel.registerPreset(') == 1
    assert 'data-presets="lilyco-react"' in html
    offline = inline_scripts(html)
    out = GEN / ('life-' + tag + '.html')
    out.write_text(offline)
    digest = hashlib.sha256(offline.encode()).hexdigest()
    print('LIFE_DRAW_FIXTURE', tag, out, len(offline.encode()), 'bytes', digest)
