#!/usr/bin/env node
// Start one Claude chat with project memory; preserve the caller's worktree.
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {spawn} from 'node:child_process';
const [project,...extra]=process.argv.slice(2);
if(!/^[a-z0-9][a-z0-9-]{0,40}$/.test(project??'')){
 console.error('Usage: node terra-memory-chat.mjs PROJECT [-- CLAUDE_ARGS...]');process.exit(1);
}
const repo=fileURLToPath(new URL('../',import.meta.url));
const folder=resolve(repo,'.local/memory',project),config=resolve(folder,'config.json');
await mkdir(folder,{recursive:true});
try{await readFile(config);}catch(error){
 if(error.code!=='ENOENT')throw error;
 const registry=JSON.parse(await readFile(resolve(repo,'.local/document-memory/registry.json'),'utf8'));
 if(!Object.hasOwn(registry.projects,project))throw Error('Register this project in the document-memory service first');
 await writeFile(config,JSON.stringify({engine:'document',url:'http://127.0.0.1:8097',project,token:registry.token}),{mode:0o600,flag:'wx'});
}
const mcp=resolve(folder,'mcp.json');
await writeFile(mcp,JSON.stringify({mcpServers:{'terra-memory':{command:process.execPath,args:[resolve(repo,'scripts/terra-document-mcp-proxy.mjs'),config]}}}));
const instruction='Use the terra-memory skill and connected terra-memory MCP as primary project memory for this chat. Verify existing file memory; save project discoveries in Terra instead of duplicating them. Do not migrate all old memory automatically. Keep personal preferences separate. Continue the user task normally.';
const child=spawn('claude',['--mcp-config',mcp,'--append-system-prompt',instruction,...(extra[0]==='--'?extra.slice(1):extra)],{stdio:'inherit'});
for(const signal of ['SIGINT','SIGTERM'])process.on(signal,()=>child.kill(signal));
child.on('error',error=>{console.error(error.message);process.exitCode=1;});
child.on('exit',(code,signal)=>{process.exitCode=code??(signal==='SIGINT'?130:143);});
