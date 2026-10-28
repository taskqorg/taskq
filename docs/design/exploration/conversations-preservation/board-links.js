'use strict';
const icons=require('./ui-icons.js');
function board(url){const p=(globalThis.reviewProjects||[]).find(p=>url===p.issues||String(url).startsWith(p.issues+'/'));const kind=p?.board_kind||p?.board?.kind;return {kind:['github','gitlab'].includes(kind)?kind:'external-link',name:kind==='github'?'GitHub':kind==='gitlab'?'GitLab':'board (provider unknown)'};}
module.exports={board,asset:url=>icons.asset(board(url).kind)};
