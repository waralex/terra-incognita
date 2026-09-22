import test from 'node:test';
import assert from 'node:assert/strict';
import {importDocument,exportDocument} from './document-model.mjs';
import {parseTree} from './parser.mjs';
const strip=n=>Array.isArray(n)?n.map(strip):n&&typeof n==='object'?Object.fromEntries(Object.entries(n).filter(([k])=>k!=='position').map(([k,v])=>[k,strip(v)])):n;
const fixtures=[
 '# <!--',
 '# Text <!-- unclosed',
 'Use [r].\n\n> [r]: https://example.com',
 'Heading\n=======\n\nText.',
 '# Title\n\nIntro **bold**.\n\n## Section\n\nSecond paragraph.\n\nThird paragraph.',
 '# Lists\n\n3. first\n4. second\n\n- [x] done\n- [ ] pending\n  - nested\n  - more',
 '- First paragraph.\n\n  Second paragraph.\n\n  ```js\n  # code\n  ```\n\n  - nested\n\n- Second item',
 '| A | B |\n| - | - |\n| x | ~~y~~ |\n\n> ## quote\n>\n> - quoted list\n\n---\n\n<script>\n# opaque\n</script>',
 '# [Title][r]\n\nUse [reference][r] and note[^n].\n\n[r]: https://example.com "Title"\n\n[^n]: Footnote.',
 '- one\n\n+ two',
 '# Unicode 😀\r\n\r\n    code();\r\n    next();\r\n',
 '- ### heading inside item\n\n  continuation',
];
for(const [i,source] of fixtures.entries())test('stable block Markdown roundtrip '+i,()=>{
 const plan=importDocument(source),blocks=plan.operations.map(o=>o.Put);
 const exported=exportDocument(blocks,plan.root);
 assert.deepEqual(strip(parseTree(exported)),strip(parseTree(source)),exported);
});
test('normalizes skipped heading levels and splits consecutive paragraphs',()=>{
 const plan=importDocument('# A\n\n### B\n\nOne.\n\nTwo.');
 assert.match(exportDocument(plan.operations.map(o=>o.Put),plan.root),/\n## B\n/);
 assert.equal(plan.operations.filter(o=>o.Put.content.kind==='Text').length,2);
});

test('parser context is not emitted inside an edited unclosed code fence',()=>{
 const plan=importDocument('# Title\n\nText.\n\n[r]: https://example.com');
 const blocks=plan.operations.map(o=>o.Put),text=blocks.find(b=>b.content.body==='Text.');
 text.content.body='```\nunclosed';
 const output=exportDocument(blocks,plan.root);
 assert.equal(parseTree(output).children.filter(n=>n.type==='definition').length,1);
 assert.equal(parseTree(output).children.find(n=>n.type==='code').value,'unclosed');
});
test('export rejects multiline titles and unrepresentable checkbox instead of dropping data',()=>{
 const plan=importDocument('# Title\n\n- [x] done'),blocks=plan.operations.map(o=>o.Put);
 const section=blocks.find(b=>b.content.title==='Title');section.content.title='Title\n\nLost text';
 assert.throws(()=>exportDocument(blocks,plan.root),/single Markdown heading/);
 section.content.title='Title';
 const item=blocks.find(b=>b.content.kind.ListItem);
 const withoutBody=blocks.filter(b=>b.content.parent!==item.id);
 assert.throws(()=>exportDocument(withoutBody,plan.root),/checkbox needs/);
});

test('setext heading retains literal trailing hashes and multiline text',()=>{
 for(const [source,title] of [['Foo ###\n===','Foo ###'],['Foo\nBar\n===','Foo Bar']]){
 const plan=importDocument(source);
 assert.equal(plan.operations.find(o=>o.Put.content.title!==null).Put.content.title,title);
 const output=exportDocument(plan.operations.map(o=>o.Put),plan.root);
 assert.equal(parseTree(output).children[0].children.map(n=>n.value??'').join(''),title);
 }
});
