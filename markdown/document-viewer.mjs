// Read-only browser gateway. The existing RPC server remains the sole DB owner.
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {localViewerRequest} from './document-viewer-access.mjs';
import {parseTree} from './parser.mjs';
const folder=resolve(process.argv[2]??'.local/document-memory');
const upstream=process.env.MEMORY_RPC_URL??'http://127.0.0.1:8097';
const routes=new Set(['read','history','search','links','backlinks']);
createServer(async(req,res)=>{
 const send=(status,value,type='application/json')=>{res.writeHead(status,{'content-type':type,'cache-control':'no-store','x-content-type-options':'nosniff'});res.end(type==='application/json'?JSON.stringify(value):value);};
 try{
  if(!localViewerRequest(req.headers))return send(403,{error:'Local requests only'});
  if(req.method!=='GET')return send(405,{error:'Read only'});
  const url=new URL(req.url,'http://localhost');
  if(url.pathname==='/')return send(200,await readFile(new URL('./document-memory.html',import.meta.url),'utf8'),'text/html; charset=utf-8');
  if(url.pathname==='/document-link-address.mjs')return send(200,await readFile(new URL('./document-link-address.mjs',import.meta.url),'utf8'),'text/javascript');
  const registry=JSON.parse(await readFile(folder+'/registry.json','utf8'));
  if(url.pathname==='/api/projects')return send(200,{projects:Object.fromEntries(Object.entries(registry.projects).map(([key,value])=>[key,{root:value.root}])),aliases:registry.aliases??{}});
  const name=url.pathname.replace(/^\/api\//,'');
  if(!url.pathname.startsWith('/api/')||!routes.has(name))return send(404,{error:'Not found'});
  const project=url.searchParams.get('project');if(!Object.hasOwn(registry.projects,project))return send(404,{error:'Unknown project'});
  const args={};for(const key of ['id','at','text','mode'])if(url.searchParams.has(key))args[key]=url.searchParams.get(key);
  if(name==='read')args.format='json';
  const response=await fetch(upstream+'/rpc',{method:'POST',headers:{'content-type':'application/json',authorization:'Bearer '+registry.token},body:JSON.stringify({project,request:{jsonrpc:'2.0',id:1,method:'tools/call',params:{name,arguments:args}}}),signal:AbortSignal.timeout(30000)});
  if(!response.ok)throw Error('Memory service HTTP '+response.status);
  const message=await response.json();if(message.error||message.result?.isError)throw Error(message.error?.message??message.result.content?.[0]?.text??'Memory read failed');
  const value=JSON.parse(message.result.content[0].text);
  if(name==='read')for(const block of value.blocks??[])block.ast=parseTree(block.body||'');
  send(200,value);
 }catch(error){send(502,{error:error.message});}
}).listen(Number(process.env.MEMORY_VIEW_PORT??8096),'127.0.0.1',()=>console.log('Read-only memory viewer listening on loopback'));
