// Lays out one bundle-only publication from a build-web.cjs output (docs/design/ui-bundle.md#contract-detail).
// Usage: node scripts/ui-bundle-stage.cjs <web-dir> <out-dir>; prints the tag and pointer name for GITHUB_OUTPUT.
'use strict';
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const [web,out]=process.argv.slice(2);
if(!web||!out)throw Error('usage: ui-bundle-stage.cjs <web-dir> <out-dir>');
const manifest=JSON.parse(fs.readFileSync(path.join(web,'manifest.json'),'utf8'));
const tag='ui-'+manifest.bundle,pointer='ui-latest-b'+manifest.bridge+'.json',sha=bytes=>crypto.createHash('sha256').update(bytes).digest('hex');
fs.mkdirSync(out,{recursive:true});
const sums=[];
for(const file of ['manifest.json',...Object.keys(manifest.files)]){
 const bytes=fs.readFileSync(path.join(web,file));
 if(file!=='manifest.json'&&(manifest.files[file].sha256!==sha(bytes)||manifest.files[file].size!==bytes.length))throw Error(file+' differs from its manifest');
 const name='taskq-ui-'+manifest.bundle+'-'+file;
 fs.writeFileSync(path.join(out,name),bytes);
 sums.push(sha(bytes)+'  '+name+'\n');
}
fs.writeFileSync(path.join(out,'SHA256SUMS'),sums.join(''));
fs.writeFileSync(path.join(out,pointer),JSON.stringify({schema:1,bundle:manifest.bundle,bridge:manifest.bridge,tag})+'\n');
process.stdout.write('tag='+tag+'\npointer='+pointer+'\n');
