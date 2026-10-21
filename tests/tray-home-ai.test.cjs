// Chain: #1427 tray AI request (menu.md M7). Risk: Send goes to no project or an unlisted one, the request row hides
// its no-start reason, or Open session links outside claude.ai.
const assert=require('node:assert/strict');
const row=(request,projects=[{root:'/a',name:'a'},{root:'/b',name:'b'}],aiRoot=null)=>{
  const context={S:{projects,ai:{history:[],request}},esc:s=>String(s).replaceAll('<','&lt;').replaceAll('"','&quot;'),aiText:'<draft>',aiMenu:false,aiRoot};
  require('./tray-harness.cjs').install(context);
  return context.ai();
};
assert(row(null).includes('data-a="ai-send" data-root="/a">'),'Send targets the first project by default');
assert(row(null).includes('&lt;draft&gt;')&&!row(null).includes('<draft>'),'the draft is escaped');
assert(row(null,undefined,'/b').includes('data-a="ai-send" data-root="/b">'),'Send targets the chosen project');
assert(row(null,undefined,'/gone').includes('data-root="/a">'),'an unlisted choice falls back to a listed project');
assert(row(null,[]).includes('data-root="" disabled'),'no project: Send disabled');
const failed=row({kind:'runtime',reason:'runtime missing: claude',next:'Install Claude Code, then send the request again',url:null});
assert(failed.includes('runtime missing: claude')&&failed.includes('send the request again')&&!failed.includes('Open session'));
assert(row({kind:'launched',url:'https://claude.ai/code/session_01Ab',next:'Open the session'}).includes('data-url="https://claude.ai/code/session_01Ab">Open session'));
assert(!row({kind:'launched',url:'https://evil.example/x',next:'n'}).includes('Open session'),'only a claude.ai session link opens');
console.log('PASS AI request: project choice, no-start reason, Open session link');
