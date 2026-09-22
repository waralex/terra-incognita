// Test client exercising the stdio MCP protocol, not direct memory methods.
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {resolve} from 'node:path';
export function connectDocumentMcp(directory,root){
 const child=spawn(process.execPath,[resolve('markdown/document-mcp.mjs'),directory,root],{stdio:['pipe','pipe','inherit']});
 let seq=0;const pending=new Map();
 const done=new Promise(resolve=>child.on('close',code=>{for(const p of pending.values())p.reject(Error('MCP closed '+code));pending.clear();resolve(code);}));
 child.on('error',error=>{for(const p of pending.values())p.reject(error);pending.clear();});
 createInterface({input:child.stdout}).on('line',line=>{const v=JSON.parse(line),p=pending.get(v.id);if(!p)return;pending.delete(v.id);v.error?p.reject(Error(v.error.message)):p.resolve(v.result);});
 const rpc=(method,params={})=>new Promise((resolve,reject)=>{const id=++seq;pending.set(id,{resolve,reject});child.stdin.write(JSON.stringify({jsonrpc:'2.0',id,method,params})+'\n');});
 return {rpc,async call(name,args={}){const r=await rpc('tools/call',{name,arguments:args});if(r.isError)throw Error(r.content[0].text);return JSON.parse(r.content[0].text);},async close(){child.stdin.end();return done;}};
}
