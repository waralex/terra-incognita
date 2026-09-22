// Canonical addresses shared by MCP and the read-only browser.
const uuid=/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export function blockHref(root,id,at){return '/?'+new URLSearchParams({root,id,...(at?{at}:{})});}
export function linkTarget(href){
 let url;try{url=new URL(href,'http://127.0.0.1:8096');}catch{return {kind:'invalid'};}
 if(!['http:','https:'].includes(url.protocol))return {kind:'external'};
 if(!['http://127.0.0.1:8096','http://localhost:8096'].includes(url.origin))return {kind:'external'};
 if(url.searchParams.has('doc'))return {kind:'legacy'};
 if(url.pathname!=='/'||!url.searchParams.has('root')||!url.searchParams.has('id'))return {kind:'external'};
 const root=url.searchParams.get('root'),id=url.searchParams.get('id'),at=url.searchParams.get('at');
 if(!uuid.test(root)||!uuid.test(id)||(at!==null&&!uuid.test(at))||['root','id','at'].some(k=>url.searchParams.getAll(k).length>1))return {kind:'invalid'};
 return {kind:'block',root:root.toLowerCase(),id:id.toLowerCase(),...(at?{at:at.toLowerCase()}:{})};
}
