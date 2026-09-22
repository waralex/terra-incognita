import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connectDocumentMcp} from './document-mcp-client.mjs';
test('stdio MCP navigation, parsed write, atomic checkbox/rename, history and scope',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-mcp-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  assert.equal((await c.rpc('initialize')).serverInfo.name,'terra-document-memory');
  assert.equal((await c.rpc('tools/list')).tools.length,8);
  const initial=await c.call('read',{format:'json'});assert.equal(initial.root,root);
  await c.call('write',{parent:root,markdown:'# Tasks\n\n- [ ] verify\n\nContext.',reason:'Track verification'});
  const top=await c.call('read',{format:'json'}),section=top.blocks.find(b=>b.title==='Tasks');
  const inside=await c.call('read',{format:'json',id:section.id}),list=inside.blocks.find(b=>b.kind.List);
  const items=await c.call('read',{format:'json',id:list.id}),item=items.blocks.find(b=>b.kind.ListItem);
  assert.equal(item.preview,'verify');
  const result=await c.call('change',{reason:'Verification passed; rename section',edits:[{id:item.id,expected:item.revision,state:'Done'},{id:section.id,expected:section.revision,title:'Verified'}]});
  const history=await c.call('history',{id:item.id});assert.equal(history[0].transaction.reason,'Verification passed; rename section');assert.equal(history[0].tx,result.tx);assert.ok(history[0].changed_blocks.includes(section.id));assert.equal(history[0].state,'Done');
  const old=await c.call('read',{format:'json',id:item.id,at:history[1].tx});assert.equal(old.blocks[0].state,'Todo');
  await assert.rejects(()=>c.call('change',{reason:'stale',edits:[{id:item.id,expected:item.revision,state:'Todo'}]}),/Stale/);
  const added=await c.call('write',{anchor:item.id,expected:result.tx,side:'after',markdown:'- [ ] follow up\n- [x] finished',reason:'Append tasks to existing list'});
  const expanded=await c.call('read',{format:'json',id:list.id});assert.equal(expanded.blocks.filter(b=>b.kind.ListItem).length,3);
  assert.deepEqual(expanded.blocks.filter(b=>b.kind.ListItem).map(b=>b.preview),['verify','follow up','finished']);
  assert.equal(added.blocks.length,2);
  await assert.rejects(()=>c.call('write',{parent:list.id,markdown:'1. wrong kind',reason:'Reject mismatched list'}),/list type/);
  const found=await c.call('search' ,{text:'Context'});assert.equal(found.hits.length,1);
  await assert.rejects(()=>c.call('read',{format:'json',id:randomUUID()}),/outside/);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});

test('empty search, depth, write manifests, safe deletion and historical recovery',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-mcp-delete-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  const empty=await c.call('search',{text:'absent'});assert.equal(empty.is_empty,true);assert.equal(empty.scanned_blocks,1);
  const created=await c.call('write',{parent:root,markdown:'# Topic\n\nOld paragraph.\n\n## Nested\n\nNested text.',reason:'Fixture'});
  assert.equal(created.written.length,4);
  const paragraph=created.written.find(b=>b.preview==='Old paragraph.');assert.ok(paragraph.id&&paragraph.revision);
  const miss=await c.call('search',{text:'absent'});assert.equal(miss.is_empty,false);assert.equal(miss.hits.length,0);
  const shallow=await c.call('read',{format:'json'});assert.equal(shallow.truncated,true);assert.equal(shallow.subtree_revision,null);
  const full=await c.call('read',{format:'json',id:created.blocks[0],depth:63});assert.equal(full.truncated,false);assert.equal(full.blocks.length,4);assert.ok(full.subtree_revision);
  await assert.rejects(()=>c.call('remove',{id:created.blocks[0],expected:created.tx,reason:'Missing scope'}),/subtree_revision/);
  await c.call('write',{target:paragraph.id,expected:paragraph.revision,markdown:'Revised paragraph.',reason:'Concurrent new conclusion'});
  await assert.rejects(()=>c.call('remove',{id:created.blocks[0],expected:created.tx,subtree_revision:full.subtree_revision,reason:'Stale subtree'}),/Subtree changed/);
  const fresh=await c.call('read',{format:'json',id:created.blocks[0],depth:63});
  const removed=await c.call('remove',{id:created.blocks[0],expected:created.tx,subtree_revision:fresh.subtree_revision,reason:'Obsolete topic'});assert.equal(removed.removed.length,4);
  assert.equal((await c.call('search',{text:'paragraph'})).is_empty,true);
  const historical=await c.call('read',{format:'json',id:created.blocks[0],at:created.tx,depth:63});assert.equal(historical.blocks.length,4);
  assert.equal((await c.call('history',{id:created.blocks[0]}))[0].deleted,true);
  const leaf=await c.call('write',{parent:root,markdown:'Disposable.',reason:'Leaf fixture'});
  await c.call('remove',{id:leaf.blocks[0],expected:leaf.tx,reason:'Remove obsolete paragraph'});
  await assert.rejects(()=>c.call('remove',{id:root,expected:created.tx,reason:'Root'}),/project root/);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});

test('child added after deletion snapshot prevents the entire removal',async()=>{
 const {openDocumentBridge}=await import('./document-bridge.mjs');
 const {DocumentMemory}=await import('./document-mcp.mjs');
 const dir=await mkdtemp(join(tmpdir(),'terra-delete-race-')),root=randomUUID();
 const bridge=openDocumentBridge(resolve('target/debug/examples/document-bridge'),dir);
 try{
  const memory=new DocumentMemory(bridge.query,{root});await memory.ensure();
  const created=await memory.call('write',{parent:root,markdown:'# Parent\n\nOriginal.',reason:'Fixture'});
  const full=await memory.call('read',{format:'json',id:created.blocks[0],depth:63});
  let raced=false;
  const racing=new DocumentMemory(async request=>{
   if(request.command==='transact'&&!raced){raced=true;await memory.call('write',{parent:created.blocks[0],markdown:'New concurrent child.',reason:'Concurrent append'});}
   return bridge.query(request);
  },{root});
  await assert.rejects(()=>racing.call('remove',{id:created.blocks[0],expected:created.tx,subtree_revision:full.subtree_revision,reason:'Attempt remove'}),/live children/);
  const after=await memory.call('read',{format:'json',id:created.blocks[0],depth:63});assert.equal(after.blocks.length,3);assert.equal(after.blocks[0].deleted,false);
 }finally{await bridge.close();await rm(dir,{recursive:true,force:true});}
});

test('UUID links, backlinks, references, historical targets and deletion',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-mcp-links-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  const created=await c.call('write',{parent:root,markdown:'# Decision\n\nOriginal decision.',reason:'Create target'});
  const section=created.blocks[0];
  const extra=await c.call('write',{parent:section,markdown:'Later evidence.',reason:'Add evidence without renaming section'});
  const target=await c.call('read',{format:'json',id:section});
  assert.equal(new URL(target.blocks[0].snapshot_href,'http://localhost').searchParams.get('at'),extra.tx);
  const pinned=target.blocks[0].snapshot_href,live=target.blocks[0].href;
  const source=await c.call('write',{parent:root,markdown:`See [decision](${live}) and [recorded decision](${pinned}).`,reason:'Reference decision'});
  let edges=await c.call('links',{id:source.blocks[0]});assert.equal(edges.links.length,2);assert.deepEqual(edges.links.map(e=>e.status),['ok','ok']);
  assert.equal((await c.call('backlinks',{id:section})).links.length,2);
  await c.call('change',{reason:'Rename target',edits:[{id:section,expected:created.tx,title:'Renamed decision'}]});
  assert.ok((await c.call('links',{id:source.blocks[0]})).links.every(e=>e.status==='ok'));
  const full=await c.call('read',{format:'json',id:section,depth:63});
  await c.call('remove',{id:section,expected:full.blocks[0].revision,subtree_revision:full.subtree_revision,reason:'Retire current decision'});
  edges=await c.call('links',{id:source.blocks[0]});assert.deepEqual(edges.links.map(e=>e.status),['deleted','ok']);
  assert.equal((await c.call('read',{format:'json',id:section})).deleted,true);
  assert.equal((await c.call('backlinks',{id:section})).links.length,2);
  const reference=await c.call('write',{parent:root,markdown:`[historical][r]\n\n[r]: ${pinned}`,reason:'Reference-style link'});
  assert.equal((await c.call('links',{id:reference.blocks[0]})).links[0].status,'ok');
  await c.call('remove',{id:source.blocks[0],expected:source.tx,reason:'Remove source'});
  assert.equal((await c.call('backlinks',{id:section})).links.length,1);
  const old=await c.call('links',{id:source.blocks[0],at:source.tx});assert.equal(old.links.length,2);assert.ok(old.links.every(e=>e.status==='ok'));
  const bad=await c.call('write',{parent:root,markdown:`[missing](/?root=${root}&id=${randomUUID()}) [other](/?root=${randomUUID()}&id=${randomUUID()}) [external](https://example.com)`,reason:'Target classifications'});
  assert.deepEqual((await c.call('links',{id:bad.blocks[0]})).links.map(e=>e.status),['missing','outside_scope','external']);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});

test('default Markdown reads do not duplicate bodies and retain editable metadata',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-read-format-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  const created=await c.call('write',{parent:root,markdown:'# Topic\n\nUnique paragraph content.\n\n- [ ] Unique task content.',reason:'Read format fixture'});
  const md=await c.call('read',{id:created.blocks[0],depth:63});
  assert.equal('blocks' in md,false);
  assert.equal(md.markdown.split('Unique paragraph content.').length,2);
  assert.equal(md.markdown.split('Unique task content.').length,2);
  assert.match(md.markdown,/kind Text parent/);assert.match(md.markdown,/state Todo/);
  const structured=await c.call('read',{id:created.blocks[0],depth:63,format:'json'});
  assert.equal('markdown' in structured,false);
  assert.equal(md.subtree_revision,structured.subtree_revision);
  const paragraph=structured.blocks.find(b=>b.body==='Unique paragraph content.');
  const match=md.markdown.match(new RegExp('block ('+paragraph.id+') revision ([0-9a-f-]+)'));
  assert.ok(match);
  await c.call('write',{target:match[1],expected:match[2],markdown:'Updated from Markdown metadata.',reason:'Verify expected unchanged'});
  const list=structured.blocks.find(b=>b.kind.List);
  const shallow=await c.call('read',{id:list.id});assert.equal(shallow.markdown.split('Unique task content.').length,2);
  await assert.rejects(()=>c.call('read',{format:'both'}),/Invalid format/);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});

test('session checkpoints stay collapsed at root and update atomically with recoverable history',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-checkpoint-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  const sessions=(await c.call('write',{parent:root,markdown:'# Сессии',reason:'Organize session checkpoints'})).blocks[0];
  const old='**Goal:** Repair cache expiry.\n**Constraint:** Do not deploy.\n**State:** TTL explanation remains unverified.\n**Next:** Check clock alignment.';
  const receipt=await c.call('write',{parent:sessions,markdown:'# Cache investigation\n\n'+old,reason:'Save before manual clear'});
  const session=receipt.blocks[0];
  const initial=await c.call('read',{id:session,depth:2,format:'json'});
  const text=initial.blocks.find(b=>b.kind==='Text');assert.ok(text);
  assert.equal(initial.blocks.length,2);
  const shallow=await c.call('read',{});assert.match(shallow.markdown,/Сессии/);assert.doesNotMatch(shallow.markdown,/Cache investigation|Do not deploy|TTL explanation/);
  const next='**Goal:** Repair cache expiry.\n**Constraint:** Do not deploy.\n**State:** Clock skew reproduced; TTL unchanged.\n**Next:** Add a clock-skew regression test.';
  await c.call('write',{target:text.id,expected:text.revision,markdown:next,reason:'Correct diagnosis from controlled reproduction'});
  await assert.rejects(c.call('write',{target:text.id,expected:text.revision,markdown:old,reason:'Stale writer'}));
  const fresh=await c.call('read',{id:session,depth:2,format:'json'});
  assert.equal(fresh.blocks.length,2);assert.equal(fresh.blocks[1].id,text.id);assert.equal(fresh.blocks[1].body,next);
  const history=await c.call('history',{id:text.id});assert.ok(history.some(v=>v.body===old));
  const pinned=await c.call('read',{id:session,at:initial.snapshot_tx,depth:2});assert.match(pinned.markdown,/TTL explanation/);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});

test('MCP search uses plain Markdown text and explicit regexp mode',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-search-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  await c.call('write',{parent:root,markdown:'A **retry** [policy](https://private.example).',reason:'Search fixture'});
  assert.equal((await c.call('search',{text:'retry policy'})).hits.length,1);
  assert.equal((await c.call('search',{text:'private.example'})).hits.length,0);
  const result=await c.call('search',{text:'RETRY\\s+policy|timeout',mode:'regex'});
  assert.equal(result.mode,'regex');assert.equal(result.hits.length,1);
  await assert.rejects(c.call('search',{text:'[',mode:'regex'}),/Invalid regexp/);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});

test('deep entrypoints appear in root overview, survive edits and retain historical state',async()=>{
 const dir=await mkdtemp(join(tmpdir(),'terra-entry-')),root=randomUUID(),c=connectDocumentMcp(dir,root);
 try{
  const receipt=await c.call('write',{parent:root,markdown:'# Area\n\n## Detail\n\nHidden content.',reason:'Fixture'});
  const leaf=receipt.written.find(b=>b.kind==='Text');
  const marked=await c.call('change',{edits:[{id:leaf.id,expected:leaf.revision,entrypoint:'Read before changing retries'}],reason:'Useful start point'});
  const overview=await c.call('read',{});
  assert.equal(overview.entrypoints.items[0].id,leaf.id);assert.doesNotMatch(overview.markdown,/Hidden content/);
  const current=await c.call('read',{id:leaf.id,format:'json'});
  await c.call('write',{target:leaf.id,expected:current.blocks[0].revision,markdown:'Updated content.',reason:'Update text'});
  const edited=await c.call('read',{id:leaf.id,format:'json'});assert.equal(edited.blocks[0].entrypoint,'Read before changing retries');
  await assert.rejects(c.call('change',{edits:[{id:leaf.id,expected:leaf.revision,entrypoint:null}],reason:'Stale removal'}));
  await c.call('change',{edits:[{id:leaf.id,expected:edited.blocks[0].revision,entrypoint:null}],reason:'No longer a starting point'});
  assert.equal((await c.call('read',{})).entrypoints.items.length,0);
  assert.equal((await c.call('read',{at:marked.tx})).entrypoints.items.length,1);
 }finally{await c.close();await rm(dir,{recursive:true,force:true});}
});
