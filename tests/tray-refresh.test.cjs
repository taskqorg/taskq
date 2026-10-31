// M3.26 / #2028 A4–A5: replay one native refresh through the real tray controller.
// Risk: intermediate snapshots/receipts emit a fitted native view which the refresh immediately removes.
const assert=require('node:assert/strict'),{install,html}=require('./tray-harness.cjs');
const display=require('./fixtures/display-model.json').display;
function rows(v){
 if(v==null||typeof v!=='object')return [];
 if(Array.isArray(v))return v.flatMap(rows);
 if(typeof v.type==='function')return rows(v.type(v.props));
 const p=v.props||{},selected=p['data-card']||p.role==='status'||p.role==='alert'||/(?:^| )(?:filter-header|project-header|task-row)(?: |$)/.test(p.class||'');
 return [...(selected?[html(v)]:[]),...rows(p.children)];
}
const project=(name,tasks=[])=>({root:name,name,issues:'https://example.org/'+name+'/issues',board_kind:'github',alive:true,complete:true,age:0,tasks_age_seconds:0,poll:60,focus:['all'],tasks});
const task={n:1,state:'doing',revision:1,title:'Invented stable task'};
const snapshot=projects=>({display,projects,font:16,theme:'dark',now:1000,notify:{projects:{}},ai:{draft:''}});
for(const [name,updates] of [
 ['partial snapshot',[snapshot([{...project('Alpha',[task]),complete:false,error:'Invented incomplete read'}]),snapshot([project('Alpha',[task])])]],
 ['per-project reads',[snapshot([project('Alpha',[task])]),snapshot([project('Alpha',[task]),project('Beta',[{...task,n:2}])])]],
 ['stale line',[snapshot([{...project('Alpha',[task]),tasks_age_seconds:121}]),snapshot([project('Alpha',[task])])]],
 ['pending readback',[snapshot([project('Alpha',[{...task,sync:{phase:'sending'}}])]),snapshot([project('Alpha',[{...task,state:'review',revision:2}])]),{viewStateError:null}]],
]){
 let now=-5000;const frames=[],timers=[],context={performance:{now:()=>now},setTimeout:f=>(timers.push(f),timers.length),clearTimeout(){},send(){},render(){frames.push({at:now,body:html(app.home()),rows:rows(app.home())});}};
 const app=install(context);app.receive(snapshot([project('Alpha',[task])]));const initial=frames.pop();
 // The compatibility replay is the pre-fix native path: separate receive calls publish each intermediate fit.
 now=100;const deliver=app.batch?values=>app.receive({updates:values}):function(values){values.forEach((value,i)=>{now=100+i*1000;app.receive(value);});};deliver(updates);
 const active=new Map(initial.rows.map(row=>[row,initial.at]));
 for(const frame of frames){const next=new Set(frame.rows);for(const [row,born] of active)if(!next.has(row)){assert(frame.at-born>=2000,name+': row vanished without input after '+(frame.at-born)+' ms: '+row.slice(0,100));active.delete(row);}for(const row of next)if(!active.has(row))active.set(row,frame.at);}
 assert.equal(frames.length,1,name+': a refresh must publish only its complete view');
 assert.equal(frames[0].body,html(app.home()),name+': publication must contain the settled view');
 assert.doesNotMatch(frames[0].body,/Invented incomplete read|Data from |Sending/);
 // R12: a settled unknown result stays visible; batching never turns failure into healthy data.
 const bad=snapshot([{...project('Alpha',[task]),complete:false,error:'Invented persistent failure',tasks_age_seconds:121}]);
 now=2500;deliver([bad]);assert.match(frames.at(-1).body,/Invented persistent failure/);assert.match(frames.at(-1).body,/Data from /);
 frames.splice(1);

 // A queued release cannot resurrect an earlier generation after the next native refresh.
 now=3000;context.pressedAt=3000;app.refresh(3000);app.release();now=3100;deliver([snapshot([project('Beta',[{...task,n:3}])])]);
 for(const timer of timers.splice(0))timer();
 assert(frames.slice(1).every(f=>!f.body.includes('Alpha')),name+': deferred presentation used an old snapshot');
 if(app.batch){let before=frames.length;now=4000;context.pressedAt=4000;deliver([snapshot([project('Alpha',[task])]),snapshot([project('Beta',[task])])]);assert.equal(frames.length,before,'a batch cannot replace a pressed target');app.release();for(const timer of timers.splice(0))timer();assert.equal(frames.length,before+1);assert(!frames.at(-1).body.includes('Alpha'),'release publishes the latest complete batch');before=frames.length;assert.throws(()=>app.batch([snapshot([project('Alpha',[task])]),null]));assert.equal(frames.length,before,'an interrupted refresh must not publish its partial view');app.batch([snapshot([project('Beta',[task])])]);assert.equal(frames.length,before+1,'a fresh complete refresh can recover');}

}
// A temporary missing project inside one refresh must not persist a fallback route.
{
 const context={render(){},send(){},setTimeout(){},clearTimeout(){}},app=install(context);
 app.receive(snapshot([project('Alpha',[task])]));context.page='Alpha';
 app.receive({updates:[snapshot([]),snapshot([project('Alpha',[task])])]});
 assert.equal(context.page,'Alpha','intermediate project absence must not replace the saved destination');
}
console.log('PASS M3.26 atomic partial/stale/project/pending refresh, deferred latest view and retained destination');
