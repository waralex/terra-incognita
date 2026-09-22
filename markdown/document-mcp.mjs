#!/usr/bin/env node
// Experimental stdio MCP. One process owns one explicitly configured document DB.
import {createHash} from 'node:crypto';
import {createInterface} from 'node:readline';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {openDocumentBridge} from './document-bridge.mjs';
import {blockHref,extractBlockLinks} from './document-links.mjs';
import {DocumentMarkdown} from './document-model.mjs';
const schema=properties=>({type:'object',properties,additionalProperties:false});
const str={type:'string'};
const revision=blocks=>createHash('sha256').update(JSON.stringify(blocks.map(b=>[b.id,b.tx]).sort((a,b)=>a[0].localeCompare(b[0])))).digest('hex');
function levels(blocks,id){const depths=new Map([[id,0]]);for(const b of blocks)if(b.id!==id)depths.set(b.id,(depths.get(b.content.parent)??64)+1);return depths;}

export const tools=[
 {name:'links',description:'Outgoing Markdown links from one block (default root). at reads source snapshot. Targets report ok/deleted/missing/outside_scope/external/legacy/invalid. Reference definitions resolve across this project. Use link_templates from Markdown read, or href/snapshot_href from JSON read; add links using write.',inputSchema:schema({id:str,at:str})},
 {name:'backlinks',description:'Current source blocks in this project that link to the target ID, including pinned historical targets. Not historical source history. Bounded on-demand scan, up to 10000 blocks, 200 results.',inputSchema:{...schema({id:str}),required:['id']}},
 {name:'remove',description:'Remove a block from current memory, retaining version history. For a section/list with children, read it fully first and supply subtree_revision. Refuses changed descendants or root deletion. expected is the block revision; reason is required.',inputSchema:{...schema({id:str,expected:str,subtree_revision:str,reason:str}),required:['id','expected','reason']}},
 {name:'read',description:'Read root or section. Default format markdown returns text once with block id/revision/kind/state comments; format json returns structured blocks instead. expected is unchanged. depth defaults to 1, use depth:63 for a complete subtree (up to 10000 blocks). truncated explicitly reports omitted descendants. Complete current reads return subtree_revision for safe section deletion. at reads historical state.',inputSchema:schema({id:str,at:str,format:{enum:['markdown','json']},depth:{type:'integer',minimum:0,maximum:63}})},
 {name:'search',description:'Search current text and titles; returns block IDs plus is_empty, scanned_blocks, complete and hit truncation. Zero hits in a populated tree differs from empty memory.',inputSchema:{...schema({text:str}),required:['text']}},
 {name:'write',description:'Insert Markdown with parent (append) or anchor+expected+side (before/after), OR replace one Text block with target+expected. Automatically splits paragraphs/lists/headings. When inserting into a List, supply one Markdown list of matching ordered/unordered type; its items are inserted into the existing list. One atomic transaction; reason required. Returns written IDs/revisions for every descendant block. Containers cannot be replaced; use remove to delete.',inputSchema:{...schema({markdown:str,reason:str,target:str,expected:str,parent:str,anchor:str,side:{enum:['before','after']}}),required:['markdown','reason']}},
 {name:'change',description:'Atomically rename sections or set task checkboxes. Supply edits [{id,expected,title}] or [{id,expected,state:"Todo"|"Done"}]. Read first for revisions.',inputSchema:{...schema({reason:str,edits:{type:'array',minItems:1,maxItems:30,items:{...schema({id:str,expected:str,title:str,state:{enum:['Todo','Done']}}),required:['id','expected']}}}),required:['reason','edits']}},
 {name:'history',description:'Block versions newest first, with transaction author/reason. Use tx as read.at to inspect historical context. before is inclusive; at most 20 versions.',inputSchema:{...schema({id:str,before:str}),required:['id']}},
];
export class DocumentMemory {
 constructor(query,{root,author='agent'}){this.query=query;this.root=root;this.author=author;this.md=new DocumentMarkdown(query);}
 async ensure(){if(!await this.query({command:'get',id:this.root}))await this.query({command:'transact',author:'system',reason:'Initialize memory root',operations:[{Put:{id:this.root,expected:null,content:{parent:null,position:1024,kind:'Section',title:'Memory',body:'',state:null,deleted:false}}}]});}
 async scoped(id,at){
  let block=await this.query({command:'get',id,at}),cursor=block;
  for(let i=0;cursor&&i<65;i++){
   if(cursor.id===this.root)return block;
   if(!cursor.content.parent)break;
   cursor=await this.query({command:'get',id:cursor.content.parent,at});
  }
  throw Error('Block is missing or outside this memory root.');
 }
 async call(name,a={}){
  const tool=tools.find(t=>t.name===name);
  if(!tool)throw Error('Unknown tool.');
  if(!a||typeof a!=='object'||Array.isArray(a)||Object.keys(a).some(k=>!Object.hasOwn(tool.inputSchema.properties,k)))throw Error('Unknown or invalid arguments; consult tools/list.');
  for(const field of tool.inputSchema.required??[])if(!Object.hasOwn(a,field))throw Error('Missing '+field);
  for(const [key,value] of Object.entries(a)){const p=tool.inputSchema.properties[key];if(p.type==='string'&&typeof value!=='string'||p.enum&&!p.enum.includes(value))throw Error('Invalid '+key);}

  if(name==='links'||name==='backlinks'){
   const snapshotTx=a.at??await this.query({command:'latest_transaction'});
   const id=a.id??this.root;await this.scoped(id,snapshotTx);
   const snapshot=await this.query({command:'subtree',root:this.root,at:snapshotTx,depth:64,max_nodes:10000});
   const all=extractBlockLinks(snapshot);
   const matching=all.filter(e=>name==='links'?e.source===id:e.target.kind==='block'&&e.target.root===this.root&&e.target.id===id);
   const cache=new Map();
   const status=async target=>{
    if(target.kind!=='block')return target.kind;
    if(target.root!==this.root)return 'outside_scope';
    const key=JSON.stringify(target);if(cache.has(key))return cache.get(key);
    const b=await this.query({command:'get',id:target.id,at:target.at??snapshotTx});
    let result='missing';
    if(b){try{await this.scoped(target.id,target.at??snapshotTx);result=b.content.deleted?'deleted':'ok';}catch{result='outside_scope';}}
    cache.set(key,result);return result;
   };
   return {scope:'project',source_state:a.at??'current',snapshot_tx:snapshotTx,scanned_blocks:snapshot.length,complete:![...levels(snapshot,this.root).values()].includes(64),truncated:matching.length>200,links:await Promise.all(matching.slice(0,200).map(async e=>({...e,source_href:blockHref(this.root,e.source,snapshotTx),status:await status(e.target)})))};
  }
  if(name==='read'){
   const snapshotTx=a.at??await this.query({command:'latest_transaction'});
   const id=a.id??this.root;const current=await this.scoped(id,snapshotTx);
   if(current.content.deleted)return {root:this.root,deleted:true,historical:!!a.at,...(a.format==='json'?{blocks:[]}:{markdown:''}),note:'This block was removed. Use history(id) and read(at) to inspect earlier versions.'};
   const depth=a.depth??1;if(!Number.isInteger(depth)||depth<0||depth>63)throw Error('depth must be an integer from 0 to 63.');
   const snapshot=await this.query({command:'subtree',root:id,at:snapshotTx,depth:depth+1,max_nodes:10000});
   const depths=levels(snapshot,id),blocks=snapshot.filter(b=>depths.get(b.id)<=depth);
   const truncated=blocks.length!==snapshot.length;
   const preview=b=>snapshot.find(child=>child.content.parent===b.id&&child.content.kind==='Text')?.content.body.slice(0,200)??'';
   const common={root:this.root,id,historical:!!a.at,snapshot_tx:snapshotTx,depth,truncated,subtree_revision:!a.at&&!truncated&&blocks.length?revision(blocks):null};
   if(a.format==='json')return {...common,blocks:blocks.map(b=>({id:b.id,revision:b.tx,href:blockHref(this.root,b.id),snapshot_href:blockHref(this.root,b.id,snapshotTx),...b.content,...(b.content.kind.ListItem?{preview:preview(b)}:{})}))};
   const visible=new Set(blocks.map(b=>b.id));
   return {...common,link_templates:{href:blockHref(this.root,'BLOCK_ID'),snapshot_href:blockHref(this.root,'BLOCK_ID',snapshotTx)},markdown:blocks.map(b=>{
    const c=b.content,kind=typeof c.kind==='string'?c.kind:JSON.stringify(c.kind);
    const meta=`<!-- block ${b.id} revision ${b.tx} kind ${kind} parent ${c.parent??'none'}${c.state!==null?' state '+c.state:''} -->`;
    const first=snapshot.find(child=>child.content.parent===b.id&&child.content.kind==='Text');
    const label=first&&!visible.has(first.id)?preview(b):'';
    const body=c.title!==null?'#'.repeat(Math.min(depths.get(b.id)+1,6))+' '+c.title:c.body||(c.kind.List?'[List]':c.kind.ListItem?`[${c.state==='Done'?'x':c.state==='Todo'?' ':'-'}]${label?' '+label:''}`:'[Section]');
    return meta+'\n'+body;
   }).join('\n\n'),note:'Use block revision as expected. Link templates substitute BLOCK_ID. Increase depth or open a child ID for more detail.'};
  }
  if(name==='search'){
   if(typeof a.text!=='string'||!a.text.trim())throw Error('Supply search text.');
   const blocks=await this.query({command:'subtree',root:this.root,depth:64,max_nodes:10000});
   const hits=blocks.filter(b=>((b.content.title??'')+'\n'+b.content.body).toLowerCase().includes(a.text.toLowerCase()));
   return {root:this.root,scanned_blocks:blocks.length,is_empty:blocks.length===1&&!blocks[0].content.body,complete:![...levels(blocks,this.root).values()].includes(64),hits:hits.slice(0,50).map(b=>({id:b.id,title:b.content.title,excerpt:b.content.body.slice(0,300)})),truncated:hits.length>50};
  }
  if(name==='history'){
   await this.scoped(a.id);
   const versions=await this.query({command:'history',id:a.id,before:a.before,limit:20});
   return Promise.all(versions.map(async b=>({id:b.id,tx:b.tx,revision:b.tx,...b.content,transaction:await this.query({command:'transaction',tx:b.tx}),changed_blocks:await this.query({command:'changes',tx:b.tx})})));
  }
  if(typeof a.reason!=='string'||!a.reason.trim())throw Error('Explain why the change is needed.');
  if(name==='remove'){
   if(a.id===this.root)throw Error('Cannot remove the project root.');
   const b=await this.scoped(a.id);
   if(b.content.deleted||b.tx!==a.expected)throw Error('Stale revision; read again.');
   const blocks=await this.query({command:'subtree',root:a.id,depth:64,max_nodes:1000});
   if(blocks[0]?.tx!==a.expected)throw Error('Stale revision; read again.');
   if(!blocks.length||[...levels(blocks,a.id).values()].includes(64))throw Error('Subtree missing or too deep to remove.');
   if(blocks.length>1&&!a.subtree_revision)throw Error('Read the full subtree and supply subtree_revision.');
   if(a.subtree_revision&&revision(blocks)!==a.subtree_revision)throw Error('Subtree changed; read again before deleting.');
   const result=await this.query({command:'transact',author:this.author,reason:a.reason,operations:blocks.map(b=>({Put:{id:b.id,expected:b.tx,content:{...b.content,deleted:true}}}))});
   return {...result,removed:blocks.map(b=>b.id)};
  }
  if(name==='write'){
   if([a.target,a.parent,a.anchor].filter(x=>x!==undefined).length!==1)throw Error('Choose exactly one of target, parent, anchor.');
   await this.scoped(a.target??a.parent??a.anchor);
   if(a.target||a.anchor){if(!a.expected)throw Error('Read first and supply expected revision.');}
   if(a.side&&!a.anchor)throw Error('side requires anchor.');
   const placement=a.parent?{Last:{parent:a.parent}}:a.anchor?{[a.side==='before'?'Before':'After']:{anchor:a.anchor,expected:a.expected}}:null;
   return this.md.write(a.markdown,{author:this.author,reason:a.reason,target:a.target??null,expected:a.expected??null,placement});
  }
  if(name==='change'){
   if(!Array.isArray(a.edits)||!a.edits.length||a.edits.length>30)throw Error('Supply 1–30 edits.');
   const seen=new Set(),operations=[];
   for(const e of a.edits){
    if(seen.has(e.id))throw Error('One edit per block.');seen.add(e.id);
    const b=await this.scoped(e.id);
    if(b.content.deleted||b.tx!==e.expected)throw Error('Stale revision; read again.');
    const content={...b.content};
    if(Object.hasOwn(e,'title')&&Object.keys(e).length===3&&content.kind==='Section'){
     if(typeof e.title!=='string'||/[\r\n]/.test(e.title))throw Error('Use a single-line title.');content.title=e.title;
    }else if(Object.hasOwn(e,'state')&&Object.keys(e).length===3&&content.kind.ListItem&&['Todo','Done'].includes(e.state)){
     content.state=e.state;
    }else throw Error('Use title on a Section or state on a ListItem.');
    operations.push({Put:{id:b.id,expected:e.expected,content}});
   }
   return this.query({command:'transact',author:this.author,reason:a.reason,operations});
  }
  throw Error('Unknown tool.');
 }
}
export async function serve({binary,directory,root,author}){
 const bridge=openDocumentBridge(binary,directory),memory=new DocumentMemory(bridge.query,{root,author});
 try{
  await memory.ensure();
  for await(const line of createInterface({input:process.stdin})){
   let request;
   try{
    request=JSON.parse(line);if(request.id===undefined)continue;
    let result;
    if(request.method==='initialize')result={protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'terra-document-memory',version:'0.1.0'},instructions:'Read first. IDs are stable; revisions guard edits. write parses Markdown into blocks. Use history to recover reasons. This is an isolated experimental memory.'};
    else if(request.method==='ping')result={};
    else if(request.method==='tools/list')result={tools};
    else if(request.method==='tools/call'){
     try{result={content:[{type:'text',text:JSON.stringify(await memory.call(request.params.name,request.params.arguments))}]};}
     catch(e){result={isError:true,content:[{type:'text',text:e.message}]};}
    }else throw Error('Unknown method.');
    process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,result})+'\n');
   }catch(e){process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request?.id??null,error:{code:-32600,message:e.message}})+'\n');}
  }
 }finally{await bridge.close();}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
 const [directory,root,binary=resolve('target/debug/examples/document-bridge')]=process.argv.slice(2);
 if(!directory||!root)throw Error('Usage: node markdown/document-mcp.mjs DB_DIRECTORY ROOT_UUID [BRIDGE_BINARY]');
 await serve({directory,root,binary,author:process.env.TERRA_AUTHOR??'agent'});
}
