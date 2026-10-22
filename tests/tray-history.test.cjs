// #1586 A2-A4 / R-UI5: menu aggregation retains exact counts at real and one-bar widths.
const assert=require('node:assert/strict'),{historyRaster}=require('../src/tray-app.js');
const bins=Array.from({length:12},(_,i)=>[i+1,i%3,12-i,i%4]);
for(const [width,count] of [[3,0],[4,1],[8,1],[9,2],[13,2],[14,3],[19,4],[29,6],[58,6],[59,12],[296,12],[592,12]]){
 const {rows,bar}=historyRaster(bins,width);assert.equal(rows.length,count,'width '+width);
 if(!count)continue;
 assert(bar>=4);assert(count*bar+count-1<=width);
 for(const [i,row] of rows.entries())for(let k=0;k<4;k++)assert.equal(row[k],bins.slice(i*12/count,(i+1)*12/count).reduce((n,b)=>n+b[k],0));
 for(let k=0;k<4;k++)assert.equal(rows.reduce((n,b)=>n+b[k],0),bins.reduce((n,b)=>n+b[k],0));
}
assert.deepEqual(historyRaster([],296),{rows:[],bar:0});
console.log('PASS history divisor selection, adjacent sums, exact totals and one-bar fallback');
