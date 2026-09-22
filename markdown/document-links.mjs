// Derived Markdown links over stable block IDs. No storage/index conventions in core.
import {toMarkdown} from 'mdast-util-to-markdown';
import {parseTree} from './parser.mjs';
import {linkTarget} from './document-link-address.mjs';
export {blockHref,linkTarget} from './document-link-address.mjs';
const walk=(node,fn)=>{fn(node);for(const child of node.children??[])walk(child,fn);};
/** Resolve reference-style links using definitions throughout the supplied snapshot. */
export function extractBlockLinks(blocks){
 const sources=blocks.flatMap(b=>['title','body'].filter(field=>typeof b.content[field]==='string'&&b.content[field]).map(field=>({block:b,field,text:b.content[field]})));
 const defs=[];
 for(const s of sources)walk(parseTree(s.text),n=>{if(n.type==='definition')defs.push(toMarkdown({type:'root',children:[n]}));});
 const prefix=defs.length?defs.join('\n\n')+'\n\n<!-- link context -->\n\n':'';
 const definitions=new Map();
 walk(parseTree(prefix),n=>{if(n.type==='definition'&&!definitions.has(n.identifier))definitions.set(n.identifier,n.url);});
 const edges=[];
 for(const s of sources){
  walk(parseTree(prefix+s.text),n=>{
   if(n.position?.start.offset<prefix.length)return;
   const href=n.type==='link'?n.url:n.type==='linkReference'?definitions.get(n.identifier):null;
   if(!href)return;
   const label=[];walk(n,c=>{if(c.type==='text'||c.type==='inlineCode')label.push(c.value);});
   edges.push({source:s.block.id,source_revision:s.block.tx,field:s.field,label:label.join(''),href,target:linkTarget(href)});
  });
 }
 return edges;
}
