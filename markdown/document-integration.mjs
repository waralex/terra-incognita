// Real RocksDB test. Build: cargo build -p terra-core --example document-bridge
import assert from 'node:assert/strict';
import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {openDocumentBridge} from './document-bridge.mjs';
import {DocumentMarkdown} from './document-model.mjs';
const binary=resolve('target/debug/examples/document-bridge');
const directory=await mkdtemp(join(tmpdir(),'terra-document-md-'));
let bridge=openDocumentBridge(binary,directory);
try {
 let md=new DocumentMarkdown(bridge.query);
 const source='# Project\n\n## Tasks\n\n3. [ ] first\n4. [x] second\n   - nested\n\n## Notes\n\n[Reference][r]\n\n[r]: https://example.com\n';
 const {root,tx}=await md.import(source,{author:'integration',reason:'Import test document'});
 const original=await md.export(root);
 const tree=await bridge.query({command:'subtree',root,depth:64,max_nodes:1000});
 const tasks=tree.find(b=>b.content.title==='Tasks'),notes=tree.find(b=>b.content.title==='Notes');
 const list=tree.find(b=>b.content.parent===tasks.id&&b.content.kind.List);
 const items=tree.filter(b=>b.content.parent===list.id);
 const moved=items[0];
 const revision=await bridge.query({command:'transact',author:'integration',reason:'Complete and reorder together',operations:[
  {Place:{edit:{id:moved.id,expected:moved.tx,content:{...moved.content,state:'Done'}},placement:{After:{anchor:items[1].id,expected:items[1].tx}}}},
  {Put:{id:notes.id,expected:notes.tx,content:{...notes.content,title:'Decisions'}}},
 ]});
 const updated=await md.export(root);
 assert.match(updated,/3\. \[x\] second/);assert.match(updated,/4\. \[x\] first/);assert.match(updated,/## Decisions/);
 assert.equal(await md.export(root,tx),original);
 await assert.rejects(()=>bridge.query({command:'transact',author:'integration',reason:'Stale edit',operations:[{Put:{id:moved.id,expected:moved.tx,content:moved.content}}]}),e=>e.kind==='conflict');
 assert.equal(await md.export(root),updated);
 const history=await bridge.query({command:'history',id:moved.id,limit:10});
 assert.equal(history.length,2);assert.equal(history[0].tx,revision.tx);
 // A section move preserves IDs and automatically changes rendered heading nesting.
 const other=await bridge.query({command:'get',id:notes.id});
 await bridge.query({command:'transact',author:'integration',reason:'Nest tasks under decisions',operations:[{Place:{edit:{id:tasks.id,expected:tasks.tx,content:tasks.content},placement:{Last:{parent:other.id}}}}]});
 assert.match(await md.export(root),/## Decisions\n[\s\S]*### Tasks/);
 assert.equal((await bridge.query({command:'get',id:list.id})).tx,tx);
 await bridge.close();bridge=openDocumentBridge(binary,directory);md=new DocumentMarkdown(bridge.query);
 assert.match(await md.export(root),/### Tasks/);
 assert.equal(await md.export(root,tx),original);
 // Unified parsed edits: identity, ordering, history, atomic rejection.
 const sample=await md.import('Before.\n\nTarget.\n\nAfter.',{author:'test',reason:'Edit fixture'});
 let snapshot=await bridge.query({command:'subtree',root:sample.root,depth:64,max_nodes:1000});
 const target=snapshot.find(b=>b.content.body==='Target.');
 const before=snapshot.find(b=>b.content.body==='Before.');
 const after=snapshot.find(b=>b.content.body==='After.');
 const change=await md.write('First.\n\nSecond.\n\n- one\n- two',{author:'test',reason:'Expand target',target:target.id,expected:target.tx});
 assert.equal(change.blocks[0],target.id);
 assert.equal(await md.export(sample.root),'Before.\n\nFirst.\n\nSecond.\n\n- one\n- two\n\nAfter.\n');
 assert.equal((await bridge.query({command:'get',id:before.id})).tx,before.tx);
 assert.equal((await bridge.query({command:'get',id:after.id})).tx,after.tx);
 assert.equal(await md.export(sample.root,sample.tx),'Before.\n\nTarget.\n\nAfter.\n');
 await assert.rejects(()=>md.write('Stale.',{author:'test',reason:'Stale',target:target.id,expected:target.tx}),/revision changed/);
 const inserted=await md.write('## New section\n\nParagraph one.\n\nParagraph two.',{author:'test',reason:'Insert section',placement:{Last:{parent:sample.root}}});
 snapshot=await bridge.query({command:'subtree',root:sample.root,depth:64,max_nodes:1000});
 assert.equal(snapshot.filter(b=>b.content.parent===inserted.blocks[0]).length,2);
 const stable=await md.export(sample.root);
 const child=snapshot.find(b=>b.content.kind.ListItem);
 await assert.rejects(()=>md.write('# Invalid child',{author:'test',reason:'Reject heading under item',placement:{Last:{parent:child.id}}}));
 assert.equal(await md.export(sample.root),stable);
 const current=await bridge.query({command:'get',id:target.id});
 await md.write('Changed paragraph.',{author:'test',reason:'Simple edit',target:target.id,expected:current.tx});
 assert.equal((await bridge.query({command:'get',id:target.id})).content.body,'Changed paragraph.');
 const refs=await md.import('Old.\n\n[r]: https://example.com',{author:'test',reason:'References'});
 const refTree=await bridge.query({command:'subtree',root:refs.root,depth:64,max_nodes:1000});
 const old=refTree.find(b=>b.content.body==='Old.');
 await md.write('Use [r].',{author:'test',reason:'Reference existing definition',target:old.id,expected:old.tx});
 assert.match(await md.export(refs.root),/Use \[r\]\./);
 const oldNow=await bridge.query({command:'get',id:old.id});
 await md.write('# Section\n\nChild.',{author:'test',reason:'Structural replacement',target:old.id,expected:oldNow.tx});
 assert.match(await md.export(refs.root),/# Section\n\nChild\./);
 const bad={};bad.self=bad;
 await assert.rejects(()=>bridge.query(bad));
 assert.ok(await bridge.query({command:'get',id:old.id}));
 const raceBlock=await bridge.query({command:'get',id:after.id});
 let raced=false;
 const racing=new DocumentMarkdown(async request=>{
  if(request.command==='transact'&&!raced){
   raced=true;
   await bridge.query({command:'transact',author:'other',reason:'Concurrent edit',operations:[{Put:{id:raceBlock.id,expected:raceBlock.tx,content:{...raceBlock.content,body:'Concurrent.'}}}]});
  }
  return bridge.query(request);
 });
 await assert.rejects(()=>racing.write('Lost.\n\nExtra.',{author:'test',reason:'Race',target:raceBlock.id,expected:raceBlock.tx}),e=>e.kind==='conflict');
 const raceOutput=await md.export(sample.root);
 assert.match(raceOutput,/Concurrent\./);assert.doesNotMatch(raceOutput,/Lost\.|Extra\./);
 console.log('PASS: real DB import/export, list reorder + checkbox + rename in one transaction, stale rejection, section move, history and reopen');
} finally {await bridge.close();await rm(directory,{recursive:true,force:true});}
