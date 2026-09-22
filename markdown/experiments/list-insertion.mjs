import {connectDocumentMcp} from '../document-mcp-client.mjs';
import {mkdtemp,writeFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const directory=await mkdtemp('/tmp/terra-mcp-list-fix-'),root=randomUUID(),client=connectDocumentMcp(directory,root),log=[];
async function call(tool,args={}){const result=await client.call(tool,args);log.push({tool,args,result});return result;}
async function read(id){return (await call('read',{format:'json',id})).blocks;}
try{
 await client.rpc('tools/list');await call('read',{format:'json'});
 const created=await call('write',{parent:root,markdown:'# Tasks\n\n3. [ ] Reproduce.\n4. [ ] Verify.',reason:'List insertion regression fixture.'});
 const section=(await read(created.blocks[0]));const list=section.find(b=>b.kind.List);
 let items=(await read(list.id)).filter(b=>b.parent===list.id);const anchor=items.at(-1);
 const inserted=await call('write',{markdown:'5. [ ] Check cleanup.',anchor:anchor.id,expected:anchor.revision,side:'after',reason:'Repeat previously failing ordered checkbox item insertion.'});
 items=(await read(list.id)).filter(b=>b.parent===list.id);
 assert.equal(items.length,3);assert(items.every(b=>b.kind.ListItem));assert.equal(items[2].id,inserted.blocks[0]);assert.equal(items[2].state,'Todo');
 assert.equal((await read(items[2].id)).find(b=>b.kind==='Text').body,'Check cleanup.');
 assert.equal((await read(list.id)).find(b=>b.id===list.id).kind.List.start,3);
 // Destination item accepts a real nested list; unwrapping applies only to List parents.
 const nestedResult=await call('write',{parent:items[2].id,markdown:'- [ ] Inspect resources.\n  - close socket\n- [x] Inspect logs.',reason:'Check nested list structure remains intact.'});
 const children=await read(items[2].id),nested=children.find(b=>b.id===nestedResult.blocks[0]);assert(nested.kind.List);assert.equal(nested.kind.List.start,null);
 const nestedItems=(await read(nested.id)).filter(b=>b.parent===nested.id);assert.equal(nestedItems.length,2);assert.equal(nestedItems[1].state,'Done');
 const innerList=(await read(nestedItems[0].id)).find(b=>b.kind.List);assert(innerList);assert.equal((await read(innerList.id)).filter(b=>b.parent===innerList.id).length,1);
 // Direct matching fragment append to List should also unwrap its items.
 await call('write',{parent:list.id,markdown:'1. [x] Completed appendix.\n2. [ ] Follow-up.',reason:'Append multiple ordered items directly to List parent.'});
 assert.equal((await read(list.id)).filter(b=>b.parent===list.id).length,5);
 const before=await read(list.id);
 await assert.rejects(()=>call('write',{parent:list.id,markdown:'- Wrong list family.',reason:'Reject mismatched list kind without mutation.'}));
 assert.deepEqual(await read(list.id),before);
 console.log('PASS',JSON.stringify({directory,root,inserted:inserted.blocks,nested:nested.id}));
}finally{await writeFile('/tmp/terra-mcp-list-fix-log.json',JSON.stringify(log,null,2));await client.close();}
