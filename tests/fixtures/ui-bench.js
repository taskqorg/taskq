// `taskq tray --snapshot DIR --route bench` (docs/testing.md, UI bench): hold the tray page for `hold` s. Every `gap` ms one
// step runs in turn: a native refresh (the service pushes a fresh state), one typed character in the AI request, a click to
// Settings, a click to Home. Each step is timed from its cause to the second animation frame after it, the first frame
// that can show it. A frame that does not come in 2 s counts as a stall, not a sample. The raw samples go to `bench.json`.
function taskqBench(hold,gap){
  const out={hold,gap,snapshot:[],typing:[],input:[],stalls:0,lost:0,errors:0,steps:0,ua:navigator.userAgent};
  const post=m=>window.ipc.postMessage(JSON.stringify(m)),wait=ms=>new Promise(r=>setTimeout(r,ms));
  const frame=()=>Promise.race([new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(performance.now())))),wait(2000).then(()=>null)]);
  const timed=(list,t0)=>frame().then(t=>t===null?out.stalls++:list.push(t-t0));
  addEventListener('error',()=>out.errors++,true);addEventListener('unhandledrejection',()=>out.errors++);
  let asked=null;const receive=taskq.receive;
  taskq.receive=s=>{const r=receive(s);if(asked!==null&&s.projects){const t0=asked;asked=null;timed(out.snapshot,t0);}return r;};
  const steps=[
    ()=>{if(asked!==null)out.lost++;asked=performance.now();post({a:'bench-push'});return wait(0);},
    ()=>{const a=document.getElementById('ai');if(!a)return wait(0);a.focus();if(a.value.length>40)a.value='';const t0=performance.now();
      a.value+='x';a.dispatchEvent(new Event('input',{bubbles:true}));return timed(out.typing,t0).then(()=>a.blur());},
    ()=>{const b=document.querySelector('[data-route=settings]'),t0=performance.now();b.click();return timed(out.input,t0);},
    ()=>{const b=document.querySelector('[data-route=home]'),t0=performance.now();b.click();return timed(out.input,t0);},
  ];
  const end=performance.now()+hold*1000;
  (async()=>{
    while(performance.now()<end){await steps[out.steps++%steps.length]();await wait(gap);}
    await wait(2100);taskq.receive=receive;
    out.heap=performance.memory?performance.memory.usedJSHeapSize:null;
    post({a:'bench',result:out});
  })().catch(e=>post({a:'bench',result:{...out,failure:String(e)}}));
}
