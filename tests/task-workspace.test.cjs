// #1645 A2: one published task workspace build. Risk: the tray drifts from the released bytes, a bundled source or
// fixture changes without its pinned hash, or the release drops an asset DashQ pins.
// After an intended source change: `node tests/task-workspace.test.cjs --write` refreshes the manifest hashes.
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm'),crypto=require('node:crypto'),esbuild=require('esbuild');
const path='docs/design/task-workspace.sources.json',manifest=require('../'+path);
const js=Object.keys(esbuild.buildSync({entryPoints:['src/task-workspace.js'],bundle:true,write:false,metafile:true,outfile:'x.js'}).metafile.inputs).filter(f=>f.startsWith('src/'));
const css=['task-workspace','answer-card','project-blocks','resource-controls','app-shell','task-dashboard'].map(n=>'src/'+n+'.css');
const hash=f=>crypto.createHash('sha256').update(fs.readFileSync(f,'utf8').replace(/\r\n/g,'\n')).digest('hex');
const files=Object.fromEntries([...js.sort(),...css,'tests/fixtures/task-workspace.json'].map(f=>[f,hash(f)]));
if(process.argv[2]==='--write'){fs.writeFileSync(path,JSON.stringify({...manifest,files},null,2)+'\n');process.exit(0);}
assert.deepEqual(manifest.files,files,'source manifest is stale: run node tests/task-workspace.test.cjs --write');
const bundle=fs.readFileSync('build/web/task-workspace.js','utf8'),tray=fs.readFileSync('build/web/tray.html','utf8');
assert(tray.includes(bundle.replaceAll('</script','<\\/script')),'tray must embed the published bundle bytes');
assert(tray.includes(fs.readFileSync('build/web/task-workspace.css','utf8')),'tray must embed the published stylesheet');
const c={setTimeout,clearTimeout,crypto:globalThis.crypto};vm.createContext(c);vm.runInContext(bundle,c);
assert.equal(c.TaskqTaskWorkspace.schema,manifest.schema);
for(const key of ['createState','create'])assert.equal(typeof c.TaskqTaskWorkspace[key],'function',key);
const release=fs.readFileSync('.github/workflows/release.yml','utf8');
for(const asset of ['task-workspace.js','task-workspace.css','task-workspace-fixtures.json','task-workspace-sources.json'])assert(release.includes('dist/taskq-$VER-'+asset),'missing released asset: '+asset);
console.log('PASS task workspace: tray embeds the released bundle and stylesheet, pinned source hashes, release assets');
