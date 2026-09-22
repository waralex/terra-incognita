import {Worker,isMainThread,parentPort,workerData} from 'node:worker_threads';
import {parseTree} from './parser.mjs';

// Match visible content, not Markdown delimiters, destinations or definitions.
export function searchableText(markdown){
 function text(node){
  if(['definition','footnoteDefinition'].includes(node.type))return '';
  if(node.type==='image'||node.type==='imageReference')return node.alt??'';
  if(node.type==='html')return (node.value??'').replace(/<!--[\s\S]*?-->/g,'').replace(/<[^>]*>/g,'');
  if(node.type==='break')return '\n';
  if(node.value!==undefined)return node.value;
  const separator=['root','list','listItem','blockquote','table','tableRow'].includes(node.type)?'\n':'';
  return (node.children??[]).map(text).filter(Boolean).join(separator);
 }
 return text(parseTree(markdown??''));
}
function matchBlocks(blocks,query,mode){
 const regex=mode==='regex'?new RegExp(query,'iu'):null;
 const needle=query.toLowerCase(),hits=[];
 for(const b of blocks){
  const title=searchableText(b.content.title),body=searchableText(b.content.body);
  const value=[title,body].filter(Boolean).join('\n');
  if(regex?regex.test(value):value.toLowerCase().includes(needle))hits.push({id:b.id,title:b.content.title===null?null:title,excerpt:body.slice(0,300)});
 }
 return {hits:hits.slice(0,50),truncated:hits.length>50};
}
export async function searchBlocks(blocks,query,mode='literal'){
 if(typeof query!=='string'||!query.trim())throw Error('Supply search text.');
 if(!['literal','regex'].includes(mode))throw Error('mode must be literal or regex');
 if(mode==='literal')return matchBlocks(blocks,query,mode);
 if(query.length>2000)throw Error('Regexp is limited to 2000 characters.');
 try{new RegExp(query,'iu');}catch(error){throw Error('Invalid regexp: '+error.message);}
 // Untrusted expressions run off the RPC event loop and are terminated on timeout.
 return new Promise((resolve,reject)=>{
  const worker=new Worker(new URL(import.meta.url),{workerData:{blocks,query,mode}});
  const timer=setTimeout(()=>{worker.terminate();reject(Error('Regexp search exceeded 2 seconds; simplify the expression.'));},2000);
  worker.once('message',result=>{clearTimeout(timer);resolve(result);});
  worker.once('error',error=>{clearTimeout(timer);reject(error);});
  worker.once('exit',code=>{clearTimeout(timer);if(code!==0)reject(Error('Regexp worker stopped'));});
 });
}
if(!isMainThread&&workerData?.mode==='regex')parentPort.postMessage(matchBlocks(workerData.blocks,workerData.query,workerData.mode));
