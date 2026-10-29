'use strict';
const ui=require('./ui-runtime.js'),{execution}=ui,{h}=require('preact');
const waiting=new Map();
const api=execution.module('reply-files',{
 settle:function(id){ui.cancel(waiting.get(id));waiting.delete(id);},
 problem:function(files){const f=(files||[]).find(f=>f.pending||f.error);return f?(f.error||f.name+': copying attachment; wait before Send'):'';},
 refuse:function(kind){switch(kind){case 'read':execution.action('reply.files.refused.read',()=>{})();break;case 'storage':execution.action('reply.files.refused.storage',()=>{})();break;default:execution.action('reply.files.refused.metadata',()=>{})();}},
 read:function(files,put){
  for(const source of files){
   const id='file-'+Array.from(globalThis.crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join(''),file={id,name:source.name||'pasted-file',mime:source.type||'application/octet-stream',bytes:source.size};
   put({...file,pending:true});
   waiting.set(id,ui.defer(()=>{waiting.delete(id);api.refuse('storage');put({...file,error:file.name+': copy receipt missing; remove and select it again before sending'});},30000));
   ui.effect('reply.files.read',()=>new Promise((resolve,reject)=>{
    const r=new FileReader();ui.listen(r,'load',()=>resolve(r.result));ui.listen(r,'error',()=>reject(Error(file.name+': file unreadable; select a readable original and retry')));r.readAsDataURL(source);
   }),result=>{
    if(typeof result!=='string'){api.settle(id);api.refuse('read');put({...file,error:result.reason||file.name+': file read failed; select it again'});return;}
    ui.send({a:'reply-file',id,file,data:result.slice(result.indexOf(',')+1)});
   });
  }
 },
 events:function(add,disabled){return {onPaste:e=>{if(!disabled&&e.clipboardData?.files.length){e.preventDefault();add([...e.clipboardData.files]);}},onDragOver:e=>{if(!disabled)e.preventDefault();},onDrop:e=>{if(!disabled&&e.dataTransfer?.files.length){e.preventDefault();add([...e.dataTransfer.files]);}}};},
 controls:function(files,disabled,add,remove){return h('div',{class:'conversation-files'},(files||[]).map(f=>h('span',{key:f.id,class:'conversation-file'},f.name+' · '+f.bytes+' bytes'+(f.pending?' · Copying…':f.error?' · Not attached':''),h('button',{type:'button',disabled,'aria-label':'Remove '+f.name,onClick:()=>remove(f.id)},'×'))),h('label',{class:'attach-label',title:'Attach images or files','aria-disabled':String(disabled)},'Attach',h('input',{type:'file',multiple:true,disabled,'data-reply-files':'pick','aria-label':'Attach images or files',onChange:e=>{add([...e.currentTarget.files]);e.currentTarget.value='';}})));},
 markup:function(files,disabled){const esc=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));return '<div class="conversation-files">'+(files||[]).map(f=>'<span>'+esc(f.name)+' · '+f.bytes+' bytes <button type="button" data-qac="remove-file" data-file="'+esc(f.id)+'" aria-label="Remove '+esc(f.name)+'"'+(disabled?' disabled':'')+'>×</button></span>').join('')+'<label>Attach <input type="file" multiple data-qac="files" aria-label="Attach images or files"'+(disabled?' disabled':'')+'></label></div>';}
});
module.exports=api;
