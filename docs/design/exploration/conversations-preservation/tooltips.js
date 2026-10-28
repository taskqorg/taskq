// Review-only shared affordance for icon controls. One top-layer hint cannot be clipped by a reader.
let hint,target,timer;
function hideHint(){clearTimeout(timer);timer=null;if(hint){if(hint.matches(':popover-open'))hint.hidePopover();hint.hidden=true;}if(target){if(target.reviewTitle!==undefined){target.setAttribute('title',target.reviewTitle);delete target.reviewTitle;}const ids=(target.getAttribute('aria-describedby')||'').split(' ').filter(x=>x&&x!=='review-action-hint');if(ids.length)target.setAttribute('aria-describedby',ids.join(' '));else target.removeAttribute('aria-describedby');}target=null;}
function showHint(el){
 hideHint();if(!el.isConnected)return;const label=el.getAttribute('aria-label')||el.title;if(!label)return;
 hint??=Object.assign(document.createElement('div'),{id:'review-action-hint',popover:'manual'});hint.setAttribute('role','tooltip');hint.style.cssText='position:fixed;inset:auto;margin:0;max-width:min(280px,calc(100vw - 16px));padding:5px 8px;border:1px solid var(--edge);border-radius:5px;background:var(--surface);color:inherit;font:12px/1.4 -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;box-shadow:0 3px 12px #0004;pointer-events:none;z-index:100';
 if(!hint.isConnected)document.body.append(hint);hint.textContent=label;hint.hidden=false;if(el.hasAttribute('title')){el.reviewTitle=el.title;el.removeAttribute('title');}hint.showPopover();target=el;
 const r=el.getBoundingClientRect(),q=hint.getBoundingClientRect();hint.style.left=Math.max(8,Math.min(innerWidth-q.width-8,r.left+(r.width-q.width)/2))+'px';hint.style.top=(r.bottom+q.height+10<innerHeight?r.bottom+5:Math.max(8,r.top-q.height-5))+'px';
 if(!el.hasAttribute('aria-label'))el.setAttribute('aria-describedby',((el.getAttribute('aria-describedby')||'')+' review-action-hint').trim());
}
function hintTarget(e){return e.target.closest?.('.compact-action[aria-label],.compact-action[title],.composer-help>summary,.composer-destination>summary');}
addEventListener('pointerover',e=>{const el=hintTarget(e);if(!el||el===target)return;clearTimeout(timer);timer=setTimeout(()=>showHint(el),250);});
addEventListener('pointerout',e=>{if(!e.relatedTarget||!e.target.closest?.('.compact-action,.composer-help,.composer-destination')?.contains(e.relatedTarget))hideHint();});
addEventListener('focusin',e=>{const el=hintTarget(e);if(el&&e.target.matches(':focus-visible'))showHint(el);});
addEventListener('focusout',hideHint);addEventListener('pointerdown',hideHint,true);addEventListener('scroll',hideHint,true);addEventListener('keydown',hideHint,true);
