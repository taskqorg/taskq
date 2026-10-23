// Standard JS bundles; no parser changes product source.
'use strict';
const fs=require('node:fs'),path=require('node:path'),esbuild=require('esbuild');
const root=path.resolve(__dirname,'..');
const out=path.resolve(process.argv[2]||path.join(root,'build/web'));
const cargo=fs.readFileSync(path.join(root,'Cargo.toml'),'utf8');
const revisions=[...cargo.matchAll(/coreq(?:-instrument)? = \{ git = "https:\/\/github.com\/taskqorg\/coreq", rev = "([a-f0-9]{40})"/g)].map(m=>m[1]);
const dependency=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8')).dependencies['@taskq/coreq'];
if(revisions.length!==2||revisions[0]!==revisions[1]||dependency!=='https://codeload.github.com/taskqorg/coreq/tar.gz/'+revisions[0])throw Error('Cargo/instrumenter/JS must pin the same CoreQ revision');

fs.mkdirSync(out,{recursive:true});
require('node:child_process').execFileSync(process.execPath,[path.join(__dirname,'check-web.cjs'),out],{stdio:'inherit'});
const components={'answer-card':'TaskqAnswerCard','resource-controls':'TaskqResources','project-blocks':'TaskqProjectBlocks','finding-composer':'TaskqFindingComposer','task-controls':'TaskqTaskControls','pm-composer':'TaskqPmComposer','task-workspace':'TaskqTaskWorkspace'};
for(const [name,globalName] of Object.entries(components))esbuild.buildSync({entryPoints:[path.join(root,'src',name+'.js')],bundle:true,platform:'browser',format:'iife',globalName,outfile:path.join(out,name+'.js'),target:'es2022'});
for(const name of ['tray-guard','workspace-entry'])esbuild.buildSync({entryPoints:[path.join(root,'src',name+'.js')],bundle:true,platform:'browser',format:'iife',outfile:path.join(out,name+'.js'),target:'es2022'});
// One task workspace stylesheet: the tray embeds it and the release publishes the same file (docs/design/task-workspace.md).
fs.writeFileSync(path.join(out,'task-workspace.css'),['task-workspace','answer-card','project-blocks','resource-controls','app-shell','task-dashboard'].map(name=>fs.readFileSync(path.join(root,'src',name+'.css'),'utf8')).join('\n'));
let html=fs.readFileSync(path.join(root,'src/tray.html'),'utf8').replace('{{TASK_WORKSPACE_CSS}}',fs.readFileSync(path.join(out,'task-workspace.css'),'utf8'));
html=html.replace('{{GUARD_JS}}',fs.readFileSync(path.join(out,'tray-guard.js'),'utf8').replaceAll('</script','<\/script'));
fs.writeFileSync(path.join(out,'tray-entry.js'),fs.readFileSync(path.join(out,'task-workspace.js'),'utf8')+';\n'+fs.readFileSync(path.join(root,'src/tray-entry.js'),'utf8'));
for(const name of ['tray','workspace']){
 const page=html.replace('{{APP_JS}}',fs.readFileSync(path.join(out,name+'-entry.js'),'utf8').replaceAll('</script','<\/script')).replace('class="menu" role="menu"','class="menu'+(name==='workspace'?' workspace':'')+'"').replace('</head>',name==='workspace'?'<style>'+fs.readFileSync(path.join(root,'src/conversations.css'),'utf8')+'</style></head>':'</head>');
 if(/\{\{[A-Z_]+\}\}/.test(page))throw Error('Unresolved web asset');
 fs.writeFileSync(path.join(out,name+'.html'),page);
}
