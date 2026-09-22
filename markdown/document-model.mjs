// Markdown syntax adapter for stable-ID documents. No RocksDB or legacy properties.
import {randomUUID} from 'node:crypto';
import {toMarkdown} from 'mdast-util-to-markdown';
import {gfmToMarkdown} from 'mdast-util-gfm';
import {parseMarkdown,parseTree} from './parser.mjs';
const options={extensions:[gfmToMarkdown()],bullet:'-',listItemIndent:'one'};
const serialize=children=>toMarkdown({type:'root',children},options).trimEnd();
const tag=kind=>typeof kind==='string'?kind:Object.keys(kind)[0];

/** Fresh import only. Re-import is not a diff and must not replace existing IDs. */
export function importDocument(source,{root=randomUUID(),id=randomUUID}={}) {
 const {tree}=parseMarkdown(source),blocks=[],byId=new Map(),orders=new Map();
 function add(parent,kind,{title=null,body='',state=null}={}) {
  if(blocks.length>=1000)throw Error('Import exceeds 1000 blocks.');
  const key=parent===null?root:id();
  if(byId.has(key))throw Error('Duplicate generated block ID.');
  const position=(orders.get(parent)??0)+1024;orders.set(parent,position);
  const block={id:key,expected:null,content:{parent,position,kind,title,body,state,deleted:false}};
  blocks.push(block);byId.set(key,block);return key;
 }
 add(null,'Section');
 function flow(nodes,parent,depth=0) {
  if(depth>48)throw Error('Markdown nesting exceeds 48 containers.');
  for(const node of nodes){
   if(node.type==='list'){
    const list=add(parent,{List:{start:node.ordered?(node.start??1):null,spread:!!node.spread}});
    for(const item of node.children){
     const child=add(list,{ListItem:{spread:!!item.spread}},{state:typeof item.checked==='boolean'?(item.checked?'Done':'Todo'):null});
     flow(item.children,child,depth+2);
    }
   }else{
    const body=source.slice(node.position.start.offset,node.position.end.offset).split('\n').map((line,i)=>i?line.replace(new RegExp('^ {0,'+(node.position.start.column-1)+'}'), ''):line).join('\n');
    add(parent,'Text',{body});
   }
  }
 }
 // Sections track heading hierarchy. Skipped heading levels normalize on export.
 const stack=[{level:0,id:root}];
 let pending=[];
 const flush=()=>{flow(pending,stack.at(-1).id,stack.length-1);pending=[];};
 for(const node of tree.children){
  if(node.type!=='heading'){pending.push(node);continue;}
  flush();while(stack.at(-1).level>=node.depth)stack.pop();
  const title=node.children.length?source.slice(node.children[0].position.start.offset,node.children.at(-1).position.end.offset).replace(/\r?\n/g,' '):'';
  const section=add(stack.at(-1).id,'Section',{title});stack.push({level:node.depth,id:section});
 }
 flush();return {root,operations:blocks.map(edit=>({Put:edit}))};
}

/** Canonical export: preserves Markdown structure, not original marker spelling. */
export function exportDocument(blocks,root) {
 const live=blocks.filter(b=>!b.content.deleted),byId=new Map(),children=new Map();
 for(const b of live){
  if(byId.has(b.id))throw Error('Duplicate block ID.');byId.set(b.id,b);
  if(!Number.isSafeInteger(b.content.position))throw Error('Position exceeds JavaScript safe integer range.');
  const bucket=children.get(b.content.parent)??[];bucket.push(b);children.set(b.content.parent,bucket);
 }
 if(!byId.has(root))throw Error('Root missing.');
 for(const bucket of children.values())bucket.sort((a,b)=>a.content.position-b.content.position||a.id.localeCompare(b.id));
 // Give each text parser document-wide definitions, so references do not become
 // escaped literal brackets just because their definition lives in another block.
 const definitions=[];
 function collectDefinitions(n){
  if(['definition','footnoteDefinition'].includes(n.type))definitions.push(serialize([n]));
  for(const child of n.children??[])collectDefinitions(child);
 }
 for(const b of live)if(tag(b.content.kind)==='Text')collectDefinitions(parseTree(b.content.body));
 const context=definitions.join('\n\n');
 const parseBody=body=>{
  const prefix=context?context+'\n\n<!-- parser context boundary -->\n\n':'';
  return parseTree(prefix+body).children.filter(n=>n.position.start.offset>=prefix.length);
 };
 const visited=new Set();
 function visit(b,headingDepth=0,depth=0){
  if(depth>=64)throw Error('Export depth limit reached; refusing potentially truncated tree.');
  if(visited.has(b.id))throw Error('Cycle in document.');visited.add(b.id);
  const c=b.content,k=tag(c.kind),kids=children.get(b.id)??[];
  if(k==='Section'){
   const level=c.title===null?headingDepth:headingDepth+1;
   if(level>6)throw Error('More than six heading levels cannot be exported as Markdown.');
   if(c.title!==null&&/[\r\n]/.test(c.title))throw Error('Section title must be a single Markdown heading.');
   const heading=c.title===null?[]:parseBody('x '+c.title+'\n===');
   if(heading.length){
    if(heading.length!==1||heading[0].type!=='heading'||heading[0].children[0]?.type!=='text')throw Error('Section title must be a single Markdown heading.');
    heading[0].depth=level;
    heading[0].children[0].value=heading[0].children[0].value.replace(/^x ?/,'');
    if(!heading[0].children[0].value)heading[0].children.shift();
   }
   return [...heading,...kids.flatMap(child=>visit(child,level,depth+1))];
  }
  if(k==='Text'){
   if(kids.length)throw Error('Text cannot have children.');return parseBody(c.body);
  }
  if(k==='List')return [{type:'list',ordered:c.kind.List.start!==null,start:c.kind.List.start,spread:c.kind.List.spread,children:kids.flatMap(child=>visit(child,headingDepth,depth+1))}];
  if(k==='ListItem'){
   const content=kids.flatMap(child=>visit(child,headingDepth,depth+1));
   if(c.state!==null&&content[0]?.type!=='paragraph')throw Error('A checkbox needs a leading text paragraph for Markdown export.');
   return [{type:'listItem',checked:c.state===null?null:c.state==='Done',spread:c.kind.ListItem.spread,children:content}];
  }
  throw Error('Unknown block kind: '+k);
 }
 const ast=visit(byId.get(root));
 if(visited.size!==live.length)throw Error('Snapshot contains disconnected blocks.');
 return toMarkdown({type:'root',children:ast},options);
}

export class DocumentMarkdown {
 constructor(query){this.query=query;}
 async import(source,{author,reason,...ids}){
  const plan=importDocument(source,ids);
  const result=await this.query({command:'transact',author,reason,operations:plan.operations});
  return {root:plan.root,tx:result.tx};
 }
 /** Insert parsed Markdown at a placement, or replace one Text leaf at its revision.
  * Containers deliberately require a separate explicit subtree operation.
  */
 async write(source,{author,reason,target=null,expected=null,placement=null}){
  if((target!==null)===(placement!==null))throw Error('Choose target or placement.');
  let previous=null;
  if(target!==null){
   previous=await this.query({command:'get',id:target});
   if(!previous||previous.content.deleted)throw Error('Target is missing or deleted.');
   if(previous.tx!==expected)throw Error('Target revision changed; read it again.');
   if(tag(previous.content.kind)!=='Text')throw Error('Replacement currently accepts a Text block only; containers need explicit subtree scope.');
  }
  const plan=importDocument(source),edits=plan.operations.map(o=>o.Put);
  let top=edits.filter(e=>e.content.parent===plan.root);
  const omitted=new Set([plan.root]);
  if(placement){
   const where=Object.values(placement)[0];
   const parentId=where.parent??(await this.query({command:'get',id:where.anchor}))?.content.parent;
   const parent=parentId?await this.query({command:'get',id:parentId}):null;
   if(parent?.content.kind.List){
    if(top.length!==1||!top[0].content.kind.List)throw Error('Insert one Markdown list when adding items to a List.');
    if((top[0].content.kind.List.start===null)!==(parent.content.kind.List.start===null))throw Error('Match the existing list type (ordered or unordered).');
    const wrapper=top[0].id;omitted.add(wrapper);
    top=edits.filter(e=>e.content.parent===wrapper);
   }
  }
  if(!top.length)throw Error('Markdown contains no blocks.');
  const operations=[];
  // Keep the target identity as the first resulting block. Its previous version
  // and all newly created siblings belong to this same transaction.
  if(previous){
   const generated=top[0].id;
   top[0].id=previous.id;top[0].expected=expected;
   for(const e of edits)if(e.content.parent===generated)e.content.parent=previous.id;
   top[0].content.parent=previous.content.parent;
   top[0].content.position=previous.content.position;
   operations.push({Put:top[0]});
  }
  for(let i=previous?1:0;i<top.length;i++){
   const where=i===0?placement:{After:{anchor:top[i-1].id,expected:top[i-1].expected}};
   operations.push({Place:{edit:top[i],placement:where}});
  }
  const topIds=new Set(top.map(e=>e.id));
  for(const e of edits)if(!omitted.has(e.id)&&!topIds.has(e.id))operations.push({Put:e});
  const result=await this.query({command:'transact',author,reason,operations});
  const written=await Promise.all(operations.map(o=>o.Put??o.Place.edit).map(async e=>{const b=await this.query({command:'get',id:e.id,at:result.tx});return {id:b.id,revision:b.tx,parent:b.content.parent,kind:b.content.kind,title:b.content.title,preview:b.content.body.slice(0,160)};}));
  return {tx:result.tx,blocks:top.map(e=>e.id),written};
 }
 async export(root,at=null){
  const blocks=await this.query({command:'subtree',root,at,depth:64,max_nodes:10000});
  return exportDocument(blocks,root);
 }
}
