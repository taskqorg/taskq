// Design source manifests (#1841): docs/design/<name>.sources.json lists the pinned files and icons;
// the build derives their SHA-256 (UTF-8, LF) into build/web/<name>.sources.json, which the release publishes.
'use strict';
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const root=path.resolve(__dirname,'..');
const names=['project-blocks','finding-composer','pm-composer','task-workspace'];
const sha=data=>crypto.createHash('sha256').update(data).digest('hex');
const text=file=>fs.readFileSync(path.join(root,file),'utf8').replace(/\r\n/g,'\n');
const render=manifest=>JSON.stringify(manifest,null,2)+'\n';
function derive(name){
 const manifest=JSON.parse(text('docs/design/'+name+'.sources.json')),derived={...manifest};
 // A listed manifest pins the derived manifest, not its committed file list.
 derived.files=Object.fromEntries(manifest.files.map(file=>{const dep=/^docs\/design\/(.+)\.sources\.json$/.exec(file);return [file,sha(dep?render(derive(dep[1])):text(file))];}));
 if(manifest.icons){const {assets}=require(path.join(root,'src/project-blocks.js'));derived.icons=Object.fromEntries(manifest.icons.map(icon=>[icon,sha(assets[icon])]));}
 return derived;
}
module.exports={names,derive,render};
