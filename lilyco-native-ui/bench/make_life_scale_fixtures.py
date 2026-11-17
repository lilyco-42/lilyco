#!/usr/bin/env python3
"""Generate scalable Conway Life fixtures from the delivered generator.

Reads HEAD / CSS / JS_GAME constants out of make_life_fixtures.py via AST, so the
React 18.3.1 + Babel 7.28.4 classic stack and the game logic stay byte-identical
apart from the four sweep parameters (COLS, ROWS, CELL, DIGEST_STEPS).
Each sweep point keeps the canvas at 720 x 480 px; only the draw-op count grows.
"""
import ast
import hashlib
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE / 'make_life_fixtures.py'
FIX = HERE / 'fixtures'
V = HERE / 'vendor'
GEN = FIX / '.generated'
GEN.mkdir(exist_ok=True)

tree = ast.parse(SRC.read_text())
CONST = {}
for node in tree.body:
    if isinstance(node, ast.Assign) and isinstance(node.targets[0], ast.Name):
        if node.targets[0].id in ('HEAD', 'CSS', 'JS_GAME') and isinstance(node.value, ast.Constant):
            CONST[node.targets[0].id] = node.value.value
assert set(CONST) == {'HEAD', 'CSS', 'JS_GAME'}

# tag, cols, rows, cell: cols*cell == 720 and rows*cell == 480 in every case.
CONFIGS = [
    ('120x80x6', 120, 80, '6'),
    ('180x120x4', 180, 120, '4'),
    ('240x160x3', 240, 160, '3'),
    ('360x240x2', 360, 240, '2'),
]
for tag, cols, rows, cell in CONFIGS:
    assert cols * float(cell) == 720.0 and rows * float(cell) == 480.0, tag

def make_js(tag, cols, rows, cell):
    js = CONST['JS_GAME']
    js, n1 = re.subn(r'const COLS = \d+, ROWS = \d+, CELL = [\d.]+;',
                     'const COLS = ' + str(cols) + ', ROWS = ' + str(rows) + ', CELL = ' + cell + ';',
                     js, count=1)
    js, n2 = re.subn(r'const DIGEST_STEPS = \d+, TICK_US = \d+;',
                     'const DIGEST_STEPS = 60, TICK_US = 16667;', js, count=1)
    js, n3 = re.subn(r"'Lilyco Life - React/Web'", "'Lilyco Life - " + tag + "'", js, count=1)
    assert (n1, n2, n3) == (1, 1, 1), (tag, n1, n2, n3)
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

for tag, cols, rows, cell in CONFIGS:
    js = make_js(tag, cols, rows, cell)
    html = make_html(tag, js)
    assert 'react@18.3.1' in html and '@babel/standalone@7.28.4' in html
    assert html.count('Babel.registerPreset(') == 1
    assert 'data-presets="lilyco-react"' in html
    offline = inline_scripts(html)
    out = GEN / ('life-scale-' + tag + '.html')
    out.write_text(offline)
    digest = hashlib.sha256(offline.encode()).hexdigest()
    print('LIFE_SCALE_FIXTURE', tag, out, len(offline.encode()), 'bytes', digest)
