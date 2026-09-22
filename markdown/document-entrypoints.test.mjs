import test from 'node:test';
import assert from 'node:assert/strict';
import {DocumentMemory} from './document-mcp.mjs';
test('entrypoint discovery budget cannot disable a shallow root read',async()=>{
 const root={id:'root',tx:'tx',content:{parent:null,title:'Project',kind:'Section',body:'',state:null,deleted:false}};
 const memory=new DocumentMemory(async q=>{
  if(q.command==='latest_transaction')return 'tx';
  if(q.command==='get')return root;
  if(q.command==='subtree'&&q.depth===64)throw Object.assign(Error('subtree node budget exceeded'),{kind:'limit'});
  if(q.command==='subtree')return [root];
  throw Error('Unexpected command');
 },{root:'root'});
 const result=await memory.call('read',{depth:0});
 assert.match(result.markdown,/Project/);assert.equal(result.entrypoints.complete,false);assert.equal(result.entrypoints.truncated,true);
});
