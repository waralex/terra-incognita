import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import {connectDocumentMcp} from '../markdown/document-mcp-client.mjs';
const repo=new URL('../',import.meta.url);
test('Node proxy authenticates and preserves shared memory writes, history and scope', {timeout:30000}, async()=>{
 const folder=await mkdtemp(join(tmpdir(),'terra-transport-')),root=randomUUID(),token=randomUUID();
 let server;
 try{
  const client=connectDocumentMcp(join(folder,'db'),root);
  try{await client.call('read',{});}finally{await client.close();}
  await writeFile(join(folder,'registry.json'),JSON.stringify({token,projects:{test:{root}}}));
  const probe=createServer();probe.listen(0,'127.0.0.1');await once(probe,'listening');
  const port=probe.address().port;await new Promise(resolve=>probe.close(resolve));
  const url=`http://127.0.0.1:${port}`;
  server=spawn(process.execPath,['markdown/document-memory-server.mjs',folder],{cwd:repo,env:{...process.env,MEMORY_RPC_PORT:String(port)},stdio:['ignore','pipe','pipe']});
  let logs='';server.stderr.on('data',b=>logs+=b);server.stdout.resume();
  let ready=false;
  for(let i=0;i<100;i++){
   if(server.exitCode!==null)throw Error(logs);
   try{const r=await fetch(url);await r.text();assert.equal(r.status,403);ready=true;break;}catch{await delay(50);}
  }
  assert.ok(ready,'server ready');
  async function request(project,credential,lines){
   const config=join(folder,'config.json');await writeFile(config,JSON.stringify({project,url,token:credential}));
   const p=spawn(process.execPath,['scripts/terra-document-mcp-proxy.mjs',config],{cwd:repo,stdio:['pipe','pipe','pipe']});
   let out='',err='';p.stdout.on('data',b=>out+=b);p.stderr.on('data',b=>err+=b);
   p.stdin.end(lines+'\n');const [code]=await once(p,'close');assert.equal(code,0,err);
   return out.trim().split('\n').filter(Boolean).map(JSON.parse);
  }
  const req=(method,params={})=>JSON.stringify({jsonrpc:'2.0',id:1,method,params});
  const [listed]=await request('test',token,'{"jsonrpc":"2.0","method":"notifications/initialized"}\n'+req('tools/list'));
  assert.deepEqual(listed.result.tools.map(t=>t.name).sort(),['read','search','write','change','remove','history','links','backlinks'].sort());
  assert.ok((await request('test','wrong',req('tools/list')))[0].error);
  assert.ok((await request('other',token,req('tools/list')))[0].error);
  const malformed=await request('test',token,'{invalid\n'+req('ping'));
  assert.ok(malformed[0].error);assert.deepEqual(malformed[1].result,{});
  async function call(name,args){const [r]=await request('test',token,req('tools/call',{name,arguments:args}));assert.ok(!r.result.isError,JSON.stringify(r));return JSON.parse(r.result.content[0].text);}
  const receipt=await call('write',{parent:root,markdown:'Durable transport note.',reason:'Transport integration test'});
  const id=receipt.blocks[0];assert.match((await call('read',{id})).markdown,/Durable transport note/);
  assert.match(JSON.stringify(await call('history',{id})),/Transport integration test/);
 }finally{
  if(server&&server.exitCode===null){const done=once(server,'close');server.kill();const timer=setTimeout(()=>server.kill('SIGKILL'),5000);try{await done;}finally{clearTimeout(timer);}}
  await rm(folder,{recursive:true,force:true});
 }
});

test('chat launcher keeps worktree and forwards arguments with a Node MCP configuration',async()=>{
 const folder=await mkdtemp(join(tmpdir(),'terra-launch-')),project='test-'+randomUUID();
 const configFolder=new URL('.local/memory/'+project+'/',repo);
 const {mkdir,readFile}=await import('node:fs/promises');
 try{
  await mkdir(configFolder,{recursive:true});
  await writeFile(new URL('config.json',configFolder),JSON.stringify({project,url:'http://127.0.0.1:1',token:'test'}));
  await writeFile(join(folder,'claude'),`#!${process.execPath}\nconsole.log(JSON.stringify({args:process.argv.slice(2),cwd:process.cwd()}));process.exitCode=7;\n`,{mode:0o700});
  const child=spawn(process.execPath,[new URL('scripts/terra-memory-chat.mjs',repo).pathname,project,'--','--resume'],{cwd:folder,env:{...process.env,PATH:folder+':'+process.env.PATH},stdio:['ignore','pipe','pipe']});
  let output='',errors='';child.stdout.on('data',b=>output+=b);child.stderr.on('data',b=>errors+=b);
  const [code]=await once(child,'close');assert.equal(code,7,errors);
  const received=JSON.parse(output);assert.equal(received.cwd,await (await import('node:fs/promises')).realpath(folder));assert.equal(received.args.at(-1),'--resume');
  const mcp=JSON.parse(await readFile(new URL('mcp.json',configFolder),'utf8')).mcpServers['terra-memory'];
  assert.equal(mcp.command,process.execPath);assert.ok(mcp.args[0].endsWith('terra-document-mcp-proxy.mjs'));
 }finally{await rm(folder,{recursive:true,force:true});await rm(configFolder,{recursive:true,force:true});}
});
