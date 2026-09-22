import test from 'node:test';
import assert from 'node:assert/strict';
import {blockHref,linkTarget,extractBlockLinks} from './document-links.mjs';
const root='11111111-1111-4111-8111-111111111111',id='22222222-2222-4222-8222-222222222222';
const block=(body,title=null)=>({id,tx:root,content:{body,title}});
test('addresses distinguish internal, legacy, external and malformed links',()=>{
 assert.deepEqual(linkTarget(blockHref(root,id,root)),{kind:'block',root,id,at:root});
 assert.equal(linkTarget('https://evil.example'+blockHref(root,id)).kind,'external');
 assert.equal(linkTarget(blockHref(root,id)+'&id='+id).kind,'invalid');
 assert.equal(linkTarget('/?root=bad&id='+id).kind,'invalid');
 assert.equal(linkTarget('/?doc=md.old&path=x').kind,'legacy');
 assert.equal(linkTarget('javascript:alert(1)').kind,'external');
});
test('extract links from titles, quotes and reference definitions; ignore code',()=>{
 const href=blockHref(root,id);
 const links=extractBlockLinks([block('> [target][r]\n\n`[not a link]('+href+')`\n\n```md\n[x]('+href+')\n```','[title]('+href+')'),block('[r]: '+href)]);
 assert.equal(links.length,2);assert.deepEqual(links.map(l=>l.label),['title','target']);assert.ok(links.every(l=>l.target.kind==='block'));
});
