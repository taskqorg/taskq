'use strict';
const {h}=require('preact'),ui=require('./ui-runtime.js'),{execution}=ui;
const mounted=new WeakSet();
let zone,exact;
const MINUTE=60,HOUR=3600,DAY=86400,UNKNOWN='? unavailable';
// Every time a window writes, in one place (specq caveman/rules/indicators.md, section Time; src/time.rs for the CLI and
// tray; tests/fixtures/time-forms.json pins both). D-A19: callers convert declared seconds; native byte cursors never enter.
const time=execution.module('relative-time',{
 value:function(raw){
  if(typeof raw==='number')return raw>0&&Number.isFinite(new Date(raw).getTime())?raw:null;
  if(typeof raw!=='string')return null;
  const m=/^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.\d+)?(Z|[+-](\d\d):(\d\d))$/i.exec(raw);
  if(!m||+m[2]<1||+m[2]>12||+m[3]<1||+m[3]>new Date(Date.UTC(+m[1],+m[2],0)).getUTCDate()||+m[4]>23||+m[5]>59||+m[6]>59||+(m[8]||0)>23||+(m[9]||0)>59)return null;
  const ms=Date.parse(raw);return ms>0&&Number.isFinite(ms)?ms:null;
 },
 seconds:function(raw){return typeof raw==='number'?time.value(raw*1000):null;},
 // Age of a past moment: the largest whole unit, rounded down. A negative age reads <1m.
 age:function(seconds){
  const s=Math.max(0,seconds);
  return s<MINUTE?'<1m':s<HOUR?Math.floor(s/MINUTE)+'m':s<DAY?Math.floor(s/HOUR)+'h':Math.floor(s/DAY)+'d';
 },
 // Countdown to a future moment, rounded up so it never arrives later than the number says.
 left:function(seconds){
  if(seconds<=0)return 'now';
  if(seconds>=DAY)return Math.ceil(seconds/DAY)+'d';
  if(seconds>=5*HOUR)return Math.ceil(seconds/HOUR)+'h';
  const minutes=Math.ceil(seconds/MINUTE);
  return Math.floor(minutes/60)+':'+String(minutes%60).padStart(2,'0');
 },
 // Exact time across hosts and in public text: 2026-11-05 14:30 UTC.
 utc:function(ms){return new Date(ms).toISOString().slice(0,16).replace('T',' ')+' UTC';},
 // Exact time for this viewer: Thu Nov 05, 14:30, with the year when it is not the current one.
 local:function(ms,now=Date.now()){
  const current=Intl.DateTimeFormat().resolvedOptions().timeZone;
  if(current!==zone){zone=current;exact=new Intl.DateTimeFormat('en-US',{weekday:'short',year:'numeric',month:'short',day:'2-digit',hour:'2-digit',minute:'2-digit',hourCycle:'h23',timeZone:zone});}
  const of=at=>Object.fromEntries(exact.formatToParts(at).map(p=>[p.type,p.value])),p=of(ms);
  return p.weekday+' '+p.month+' '+p.day+(p.year===of(now).year?'':' '+p.year)+', '+p.hour+':'+p.minute;
 },
 // The clock alone while the date does not affect interpretation.
 clock:function(ms,now=Date.now()){
  const whole=time.local(ms,now),day=text=>text.slice(0,text.indexOf(','));
  return day(whole)===day(time.local(now,now))?whole.slice(whole.indexOf(',')+2):whole;
 },
 label:function(ms,now=Date.now(),left=false){return left?time.left((ms-now)/1000):time.age((now-ms)/1000);},
 detail:function(ms,context){
  const text=time.local(ms);
  return context+' · '+text+' · '+zone;
 },
 stamp:function(raw,context='Time',reason,focus=true,left=false){
  const ms=time.value(raw),full=ms===null?context+' · '+UNKNOWN+': '+(reason||(raw==null?'Source timestamp unavailable':'Source timestamp invalid or missing a time zone')):time.detail(ms,context);
  return h(ms===null?'span':'time',{class:'relative-time'+(ms===null?' time-unknown':left?' time-future':''),...(ms===null?{}:{dateTime:new Date(ms).toISOString(),'data-time-ms':ms,'data-time-source':raw,'data-time-context':context,'data-time-left':left?'':undefined}),title:full,'aria-label':full,tabIndex:focus?0:undefined},ms===null?UNKNOWN:time.label(ms,Date.now(),left));
 },
 refresh:function(root,now=Date.now()){
  for(const el of root.querySelectorAll('time[data-time-ms]')){const next=time.label(Number(el.dataset.timeMs),now,'timeLeft' in el.dataset);if(el.textContent!==next)el.textContent=next;}
 },
 mount:function(root){
  if(mounted.has(root))return;mounted.add(root);
  const document=root.ownerDocument,tip=document.createElement('div');tip.className='time-tooltip';tip.setAttribute('role','tooltip');tip.hidden=true;document.body.append(tip);
  let target=null;
  const hide=()=>{tip.hidden=true;target=null;},show=node=>{
   target=node?.closest('.relative-time,[data-tooltip]')||node?.closest('button,summary')?.querySelector('.relative-time');
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
module.exports=Object.freeze({...time,unknown:UNKNOWN});
