// Instrumentation only; disposable IPC sink never acknowledges a real operation.
(() => {
 const p=window.probe1932={input:[],selection:[],frames:[],wheel:0,keys:0,composition:0,effects:[],errors:[],mountedMax:0,wheelFrames:[],seen:new Set(),duplicate:false};
 const start=performance.now();let last=0,wheelAt=0;
 const frame=t=>{if(last&&t-start<120000){p.frames.push(t-last);if(t-wheelAt<500)p.wheelFrames.push(t-last);}last=t;if(t-start<120000)requestAnimationFrame(frame);};requestAnimationFrame(frame);
 const feedback=kind=>{const at=performance.now();requestAnimationFrame(()=>p[kind].push(performance.now()-at));};
 document.addEventListener('input',()=>feedback('input'),true);
 document.addEventListener('selectionchange',()=>feedback('selection'));
 document.addEventListener('wheel',()=>{p.wheel++;wheelAt=performance.now();},{passive:true});
 document.addEventListener('keydown',()=>p.keys++,true);
 document.addEventListener('compositionend',()=>p.composition++,true);
 window.addEventListener('error',e=>p.errors.push(e.message));
 const old=window.ipc.postMessage.bind(window.ipc);window.ipc.postMessage=t=>{const m=JSON.parse(t);if(['answer','ai','conversation-action','reply-file','conversation-dialogue'].includes(m.a))p.effects.push({a:m.a,action:m.action,id:m.id});old(t);};
 const mounted=()=>{const keys=[...document.querySelectorAll('.dialogue-transcript>[data-key]')].map(e=>e.dataset.key);p.mountedMax=Math.max(p.mountedMax,keys.length);if(new Set(keys).size!==keys.length)p.duplicate=true;keys.forEach(k=>p.seen.add(k));};new MutationObserver(mounted).observe(document.body,{childList:true,subtree:true});mounted();
 const stats=a=>{a=[...a].sort((x,y)=>x-y);return {n:a.length,p50:a[Math.floor(a.length*.5)]??null,p95:a[Math.floor(a.length*.95)]??null,max:a.at(-1)??null};};
 p.snapshot=()=>{
  const r=document.querySelector('.conversation-reader'),input=document.querySelector('.conversation-composer textarea'),send=document.querySelector('[data-a=answer-send],[data-c=reply]');
  const bounds=e=>e?.getBoundingClientRect().toJSON()??null;
  const mounted=document.querySelectorAll('.dialogue-transcript>[data-key]').length;p.mountedMax=Math.max(p.mountedMax,mounted);
  const face=input?getComputedStyle(input):null;
  return {at:Date.now(),width:innerWidth,height:innerHeight,dpr:devicePixelRatio,font:face?.fontFamily,size:face?.fontSize,theme:document.documentElement.dataset.theme,
   rootOverflow:document.documentElement.scrollWidth-innerWidth,input:input?{value:input.value,client:input.clientHeight,scroll:input.scrollHeight,resize:face.resize}:null,
   send:bounds(send),reader:r?{top:r.scrollTop,height:r.clientHeight,scroll:r.scrollHeight}:null,followPaused:!!document.querySelector('[data-c=latest]'),
   mounted,mountedMax:p.mountedMax,totalNodes:document.getElementsByTagName('*').length,inputFeedbackMs:stats(p.input),selectionFeedbackMs:stats(p.selection),frameMs:stats(p.frames),
   wheelFrames:stats(p.wheelFrames),renderReceiveMs:stats(window.history1742?.renders||[]),seen:p.seen.size,duplicate:p.duplicate,longTaskApi:PerformanceObserver.supportedEntryTypes.includes("longtask"),
   anchor:(()=>{const top=r?.getBoundingClientRect().top;const e=[...document.querySelectorAll(".dialogue-transcript>[data-key]")].find(e=>e.getBoundingClientRect().bottom>top);return e?{key:e.dataset.key,y:e.getBoundingClientRect().top-top}:null;})(),
   wheel:p.wheel,keys:p.keys,composition:p.composition,effects:p.effects,errors:p.errors,
   ariaLive:document.querySelector('.dialogue-transcript')?.getAttribute('aria-live')??null};
 };
 if(typeof p.snapshot!=='function'||!Array.isArray(p.input))throw Error('1932 instrument self-check');
})();
