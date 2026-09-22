import test from 'node:test';
import assert from 'node:assert/strict';
import {parseMarkdown} from './parser.mjs';

const bodies=source=>{const p=parseMarkdown(source);return p.tree.children.flatMap(n=>n.type==='heading'?[]:n.type==='list'?n.children.map(p.slice):[p.slice(n)]);};

test('loose list items retain paragraphs, nested list, fenced code and sibling boundary',()=>{
 const first='- First paragraph.\n\n  Second paragraph.\n\n  - Nested item\n\n  ```js\n  # not a heading\n  ```';
 const source='# Title\n\n'+first+'\n\n- Second item';
 assert.deepEqual(bodies(source),[first,'- Second item']);
 const ast=parseMarkdown(source).tree.children[1];
 assert.equal(ast.type,'list');assert.equal(ast.spread,true);
 assert.deepEqual(ast.children[0].children.map(n=>n.type),['paragraph','paragraph','list','code']);
});
test('Setext headings become sections; headings inside containers stay in content',()=>{
 const source='Title\n=====\n\nSection\n-------\n\n> ## Quoted heading\n>\n> paragraph\n\n- ### Item heading\n\n  continuation';
 const p=parseMarkdown(source);
 assert.deepEqual(p.tree.children.filter(n=>n.type==='heading').map(n=>n.children[0].value),['Title','Section']);
 assert.equal(bodies(source).length,2);
 assert.match(bodies(source)[0],/^> ## Quoted/);
});
test('GFM tables, tasks, strikethrough and footnotes retain structure and source',()=>{
 const table='| A | B |\n| - | - |\n| x | y |';
 const source=table+'\n\n- [x] ~~done~~\n- [ ] pending\n\nReference[^n].\n\n[^n]: explanation';
 const parsed=parseMarkdown(source);
 assert.deepEqual(parsed.tree.children.map(n=>n.type),['table','list','paragraph','footnoteDefinition']);
 assert.equal(parsed.tree.children[1].children[0].checked,true);
 assert.equal(parsed.tree.children[1].children[0].children[0].children[0].type,'delete');
 assert.equal(bodies(source)[0],table);
 assert.ok(bodies(source).includes('[^n]: explanation'));
});
test('CRLF, Unicode and indented code use original source coordinates',()=>{
 const source='# Пример\r\n\r\n- 😀 first\r\n\r\n  продолжение\r\n\r\n    code();\r\n';
 const parsed=parseMarkdown(source),list=parsed.tree.children[1];
 assert.match(parsed.slice(list.children[0]),/😀 first\r\n\r\n  продолжение/);
 assert.equal(parsed.source,source);
 assert.equal(bodies('    code();\n    next();')[0],'    code();\n    next();');
});
test('HTML and reference definitions stay opaque; importing cannot execute content',()=>{
 const source='<script>\n# ignored heading\nalert(1)\n</script>\n\n[link][r]\n\n[r]: https://example.com "Title"';
 assert.deepEqual(parseMarkdown(source).tree.children.map(n=>n.type),['html','paragraph','definition']);
 assert.deepEqual(bodies(source),['<script>\n# ignored heading\nalert(1)\n</script>','[link][r]','[r]: https://example.com "Title"']);
});
test('ordered list numbering and lazy continuation are preserved',()=>{
 const source='3. First\ncontinuation\n\n   Another paragraph\n\n4. Second';
 const parsed=parseMarkdown(source);
 assert.equal(parsed.tree.children[0].start,3);
 assert.deepEqual(bodies(source),['3. First\ncontinuation\n\n   Another paragraph','4. Second']);
});
