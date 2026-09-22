// Local experimental process transport. stdout is JSONL; stderr stays diagnostic.
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
export function openDocumentBridge(binary,directory){
 const child=spawn(binary,[directory],{stdio:['pipe','pipe','inherit']}),pending=[];
 let failure=null;
 function fail(error){failure=error;for(const p of pending.splice(0))p.reject(error);}
 child.on('error',fail);child.stdin.on('error',fail);
 const done=new Promise(resolve=>child.on('close',(code)=>{fail(Error('Document bridge closed: '+code));resolve(code);}));
 createInterface({input:child.stdout}).on('line',line=>{
  const waiter=pending.shift();if(!waiter)return fail(Error('Unexpected bridge response.'));
  try{const v=JSON.parse(line);if(v.error){const e=Error(v.error.message);e.kind=v.error.kind;waiter.reject(e);}else waiter.resolve(v.ok);}catch(e){waiter.reject(e);fail(e);}
 });
 return {
  query(body){if(failure)return Promise.reject(failure);return new Promise((resolve,reject)=>{const line=JSON.stringify(body)+'\n';pending.push({resolve,reject});child.stdin.write(line);});},
  async close(){child.stdin.end();return done;},
 };
}
