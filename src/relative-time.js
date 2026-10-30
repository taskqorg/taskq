'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const mounted=new WeakSet(),relative=new Intl.RelativeTimeFormat('en',{numeric:'always',style:'short'});
let zone,exact;
// D-A19: callers convert declared seconds; native byte cursors never enter this renderer.
const time=execution.module('relative-time',{
 value:function(raw){
  if(typeof raw==='number')return raw>0&&Number.isFinite(new Date(raw).getTime())?raw:null;
  if(typeof raw!=='string')return null;
  const m=/^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.\d+)?(Z|[+-](\d\d):(\d\d))$/i.exec(raw);
  if(!m||+m[2]<1||+m[2]>12||+m[3]<1||+m[3]>new Date(Date.UTC(+m[1],+m[2],0)).getUTCDate()||+m[4]>23||+m[5]>59||+m[6]>59||+(m[8]||0)>23||+(m[9]||0)>59)return null;
  const ms=Date.parse(raw);return ms>0&&Number.isFinite(ms)?ms:null;
 },
 seconds:function(raw){return typeof raw==='number'?time.value(raw*1000):null;},
 label:function(ms,now=Date.now()){
  const delta=(ms-now)/1000,age=Math.abs(delta);
  if(age<60)return delta>0?'in a moment':'just now';
  const [unit,size]=age<3600?['minute',60]:age<86400?['hour',3600]:['day',86400];
  return relative.format(Math.sign(delta)*Math.floor(age/size),unit);
 },
 detail:function(ms,context){
  const current=Intl.DateTimeFormat().resolvedOptions().timeZone;
  if(current!==zone){zone=current;exact=new Intl.DateTimeFormat('en-GB',{year:'numeric',month:'short',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit',hourCycle:'h23',timeZone:zone,timeZoneName:'longOffset'});}
  return context+' · '+exact.format(ms)+' · '+zone;
 },
 stamp:function(raw,context='Time',reason,focus=true){
  const ms=time.value(raw),full=ms===null?context+' · Time unknown: '+(reason||(raw==null?'Source timestamp unavailable':'Source timestamp invalid or missing a time zone')):time.detail(ms,context);
  return h(ms===null?'span':'time',{class:'relative-time'+(ms===null?' time-unknown':''),...(ms===null?{}:{dateTime:new Date(ms).toISOString(),'data-time-ms':ms,'data-time-source':raw,'data-time-context':context}),title:full,'aria-label':full,tabIndex:focus?0:undefined},ms===null?'Time unknown':time.label(ms));
 },
 refresh:function(root,now=Date.now()){
  for(const el of root.querySelectorAll('time[data-time-ms]')){const next=time.label(Number(el.dataset.timeMs),now);if(el.textContent!==next)el.textContent=next;}
 },
 mount:function(root){
  if(mounted.has(root))return;mounted.add(root);
  const document=root.ownerDocument,tip=document.createElement('div');tip.className='time-tooltip';tip.setAttribute('role','tooltip');tip.hidden=true;document.body.append(tip);
  let target=null;
  const hide=()=>{tip.hidden=true;target=null;},show=node=>{
   target=node?.closest('.relative-time')||node?.closest('button,summary')?.querySelector('.relative-time');
   if(!target)return hide();
   if(target.dataset.timeMs){target.title=time.detail(Number(target.dataset.timeMs),target.dataset.timeContext);target.setAttribute('aria-label',target.title);}
   tip.textContent=target.title;tip.hidden=false;
   const r=target.getBoundingClientRect();tip.style.left=Math.max(4,Math.min(r.left,document.documentElement.clientWidth-tip.offsetWidth-4))+'px';tip.style.top=Math.max(4,Math.min(r.bottom+4,document.documentElement.clientHeight-tip.offsetHeight-4))+'px';
  };
  ui.listen(root,'focusin',e=>show(e.target));ui.listen(root,'focusout',hide);
  ui.listen(root,'mouseover',e=>show(e.target));ui.listen(root,'mouseout',e=>{if(!tip.contains(e.relatedTarget))hide();});
  ui.listen(root,'keydown',e=>{if(e.key==='Escape')hide();});ui.listen(root,'scroll',hide,true);
  const tick=()=>{if(!root.isConnected){tip.remove();return;}time.refresh(root);if(target&&!target.isConnected)hide();ui.defer(tick,60000-Date.now()%60000+1);};tick();
 }
});
module.exports=time;
