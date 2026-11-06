// Load before product modules: a broken renderer must not keep apparently usable controls.
// #1680 A3: the site is code identity only (error class, function, line:col, element tag); messages and URLs can carry content.
function site(error){
  const name=typeof error?.name==='string'&&/^\w{1,40}$/.test(error.name)?error.name:'unknown';
  const frame=String(error?.stack||'').split('\n').map(l=>l.trim()).find(l=>/:\d+:\d+\)?$/.test(l))||'';
  const at=frame.match(/:(\d{1,9}):(\d{1,9})\)?$/),fn=(frame.match(/^at ([\w$.]{1,60}) \(/)||frame.match(/^([\w$.]{1,60})@/)||[])[1];
  return {error:name,site:(fn||'anonymous')+(at?':'+at[1]+':'+at[2]:'')};
}
function install(window=globalThis){
const document=window.document;
const counts=[0,0,0,0],classes=['script','rejection','resource','evaluation'];
let dirty=false,timer=0,shown=false,saturated=false,last=null;
function notice(){
  if(shown||!document.body)return;shown=true;
  const style=document.createElement('style');style.textContent='.menu,.workspace{display:none!important}';document.head.append(style);
  const box=document.createElement('div');box.setAttribute('role','alert');
  box.style.cssText='box-sizing:border-box;padding:12px;max-width:100%;overflow-wrap:anywhere';
  box.textContent='Interface error; command outcomes unknown. Close and reopen TaskQ. Check service status before retrying a command.';
  if(document.querySelector?.('.workspace')){
    box.textContent='Interface error; command outcomes unknown. Drafts and receipts are retained. Use native Close or Reload Conversations safely. Check delivery before retrying a command.';
    for(const [label,message] of [['Reload safely',{a:'workspace-reload'}],['Back to safe view',{a:'workspace-reload',safe:true}],['Close',{a:'hide'}]]){
      const button=document.createElement('button');button.textContent=label;
      button.addEventListener('click',()=>{try{window.ipc.postMessage(JSON.stringify(message));}catch(_){box.textContent='Recovery bridge unavailable. Use the native Conversations menu or window Close.';}});box.append(button);
    }
  }
  document.body.append(box);
}
function flush(){
  timer=0;if(!dirty)return;dirty=false;
  try{window.ipc.postMessage(JSON.stringify({a:'web-error',counts:counts.slice(),saturated,...(last||{})}));}catch(_){/* native bootstrap/evaluation deadlines expose a missing bridge */}
  timer=window.setTimeout(flush,2000);
}
window.taskqWebFault=(kind,detail)=>{
  const i=classes.indexOf(kind);if(i>=0){if(counts[i]<4294967295)counts[i]++;else saturated=true;dirty=true;if(detail&&!last)last=detail;}
  notice();if(dirty&&!timer)flush();
};
window.addEventListener('DOMContentLoaded',()=>{if(counts.some(Boolean))notice();});
window.addEventListener('error',e=>{
  if(e.target===window){const d=e.error?site(e.error):{error:'unknown',site:'anonymous'};if(!e.error&&Number.isInteger(e.lineno))d.site+=':'+e.lineno+':'+(e.colno|0);window.taskqWebFault('script',d);}
  else window.taskqWebFault('resource',{error:'resource',site:/^[a-z]{1,20}$/i.test(e.target?.tagName||'')?e.target.tagName.toLowerCase():'element'});
},true);
window.addEventListener('unhandledrejection',e=>window.taskqWebFault('rejection',e.reason instanceof Error?site(e.reason):{error:'rejection',site:'promise'}));
}
module.exports={install,site};
