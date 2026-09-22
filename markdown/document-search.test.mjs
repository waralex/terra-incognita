import test from 'node:test';
import assert from 'node:assert/strict';
import {searchableText,searchBlocks} from './document-search.mjs';
const block=body=>({id:'1',content:{title:'**Cache**',body}});
test('search removes syntax and link destinations while preserving readable code and labels',async()=>{
 const body='The **retry** [policy](https://secret.example) uses `x_y`.\n\n```js\nx += 1;\n```\n\n<!-- hidden -->';
 assert.match(searchableText(body),/The retry policy uses x_y/);
 assert.equal((await searchBlocks([block(body)],'retry policy')).hits.length,1);
 assert.equal((await searchBlocks([block(body)],'secret.example')).hits.length,0);
 assert.equal((await searchBlocks([block(body)],'hidden')).hits.length,0);
 assert.equal((await searchBlocks([block(body)],'x += 1')).hits.length,1);
});
test('regexp is explicit, case insensitive, and rejects invalid patterns',async()=>{
 const blocks=[block('retry failed')];
 assert.equal((await searchBlocks(blocks,'CACHE|timeout','regex')).hits.length,1);
 assert.equal((await searchBlocks(blocks,'CACHE|timeout')).hits.length,0);
 await assert.rejects(searchBlocks(blocks,'[','regex'),/Invalid regexp/);
});
test('pathological regexp is bounded',async()=>{
 await assert.rejects(searchBlocks([block('a'.repeat(10000)+'!')],'(a+)+$','regex'),/exceeded|stopped/);
});
