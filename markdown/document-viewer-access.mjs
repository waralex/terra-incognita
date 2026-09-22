// Do not expose the credential-bearing local proxy to arbitrary browser origins.
export function localViewerRequest(headers){
 const host=headers.host;
 if(typeof host!=='string'||!/^(localhost|127\.0\.0\.1)(:\d+)?$/.test(host))return false;
 if(headers['sec-fetch-site']==='cross-site')return false;
 if(headers.origin!==undefined&&headers.origin!==`http://${host}`)return false;
 return true;
}
