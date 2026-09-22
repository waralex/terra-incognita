// Shared local owner of the document DB for authenticated MCP clients.
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {openDocumentBridge} from './document-bridge.mjs';
import {DocumentMemory,tools} from './document-mcp.mjs';
const folder=resolve(process.argv[2]??'.local/document-memory');
const registry=JSON.parse(await readFile(folder+'/registry.json','utf8'));
const bridge=openDocumentBridge(resolve('target/debug/examples/document-bridge'),folder+'/db');
const memories=new Map(Object.entries(registry.projects).map(([project,p])=>[project,new DocumentMemory(bridge.query,{root:p.root,author:'agent:'+project})]));
async function call(project,name,args={}){
 if(!memories.has(project))throw Error('Unknown project');
 return memories.get(project).call(name,args);
}
const send=(res,status,value,type='application/json')=>{res.writeHead(status,{'content-type':type,'cache-control':'no-store','x-content-type-options':'nosniff'});res.end(type==='application/json'?JSON.stringify(value):value);};
const rpc=createServer(async(req,res)=>{
 try{
  if(req.method!=='POST'||req.url!=='/rpc'||req.headers.authorization!=='Bearer '+registry.token)return send(res,403,{error:'Unauthorized'});
  let text='';for await(const chunk of req){text+=chunk;if(text.length>1000000)return send(res,413,{error:'Request too large'});}
  const {project,request:r}=JSON.parse(text);if(!memories.has(project))throw Error('Unknown project');
  let result;
  if(r.method==='initialize')result={protocolVersion:'2024-11-05',capabilities:{tools:{}},serverInfo:{name:'terra-memory',version:'2'},instructions:'Read root first. Current memory uses stable block IDs and revisions.'};
  else if(r.method==='tools/list')result={tools};
  else if(r.method==='ping')result={};
  else if(r.method==='tools/call'){try{result={content:[{type:'text',text:JSON.stringify(await call(project,r.params.name,r.params.arguments))}]};}catch(e){result={isError:true,content:[{type:'text',text:e.message}]};}}
  else throw Error('Unknown method');
  send(res,200,{jsonrpc:'2.0',id:r.id,result});
 }catch(e){send(res,400,{error:e.message});}
});
await new Promise((ok,no)=>rpc.once('error',no).listen(Number(process.env.MEMORY_RPC_PORT??8097),'127.0.0.1',ok));
console.log('Document memory RPC listening on loopback');
for(const signal of ['SIGTERM','SIGINT'])process.on(signal,()=>{rpc.close();bridge.close().then(()=>process.exit(0));});
