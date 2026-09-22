#!/usr/bin/env node
// Thin stdio MCP transport to the shared document-memory owner.
import {readFile} from 'node:fs/promises';
import {createInterface} from 'node:readline';
const config=JSON.parse(await readFile(process.argv[2],'utf8'));
for await(const line of createInterface({input:process.stdin,crlfDelay:Infinity})){
 let request,result;
 try{
  request=JSON.parse(line);
  if(!request||typeof request!=='object'||Array.isArray(request))throw Error('Invalid request');
  if(!Object.hasOwn(request,'id'))continue;
  const response=await fetch(config.url+'/rpc',{
   method:'POST',headers:{'Content-Type':'application/json',Authorization:'Bearer '+config.token},
   body:JSON.stringify({project:config.project,request}),signal:AbortSignal.timeout(45000),
  });
  if(!response.ok)throw Error('Memory service HTTP '+response.status);
  result=await response.json();
 }catch(error){result={jsonrpc:'2.0',id:request?.id??null,error:{code:-32603,message:error.message}};}
 process.stdout.write(JSON.stringify(result)+'\n');
}
