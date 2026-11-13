//! The drawing viewer page for the WebView.
//!
//! The page loads once. Rust sends the SVG over the bridge (`WebViewRef::post_message`)
//! as `fit:<svg>` (a new drawing or sheet: fit it to the screen) or `keep:<svg>` (an edit:
//! keep zoom and position). When the page is ready for data it answers `ready`.
//!
//! Gestures: two fingers zoom around the gesture's midpoint, one finger pans, a double
//! tap fits the sheet, a single tap picks a label (dimension, text, title-block cell).
//! After a pinch the zoom is committed by resizing the SVG itself rather than by a CSS
//! transform, so lines stay sharp at any magnification.

pub const READY: &str = "ready";
/// The page painted the sheet it was sent (after the next frame).
pub const DRAWN: &str = "drawn";

pub fn message(fit: bool, svg: &str) -> String {
    let body = svg
        .strip_prefix("<?xml")
        .and_then(|rest| rest.find("?>").map(|end| &rest[end + 2..]))
        .unwrap_or(svg);
    format!("{}:{}", if fit { "fit" } else { "keep" }, body)
}

/// Label boxes of the current sheet for picking by tap: `hits:[[id,x0,y0,x1,y1],…]` in
/// SVG coordinates. The page answers `tap:<id>` (or `tap:` when no label is near).
pub fn hits(hits: &[crate::engine::Hit]) -> String {
    let list: Vec<serde_json::Value> = hits
        .iter()
        .map(|h| serde_json::json!([h.id, h.rect[0], h.rect[1], h.rect[2], h.rect[3]]))
        .collect();
    format!("hits:{}", serde_json::Value::Array(list))
}

/// Highlight an item (`sel:<id>`) or clear the highlight (`sel:`).
pub fn select(id: Option<&str>) -> String {
    format!("sel:{}", id.unwrap_or_default())
}

pub const TAP: &str = "tap:";

/// Empty the page (the drawing was closed).
pub const CLEAR: &str = "clear:";

/// The page, starting in the theme's background (`#RRGGBB`) so nothing flashes before the
/// `bg:` message arrives.
pub fn html(background: &str) -> String {
    HTML.replace("background:#d9d9de", &format!("background:{background}"))
}

/// Background around the sheet (`#RRGGBB`): the current theme's surface container.
pub fn background(rgb: &str) -> String {
    format!("bg:{rgb}")
}

const HTML: &str = r##"<!doctype html>
<html><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no">
<style>
  html,body{margin:0;height:100%;overflow:hidden;background:#d9d9de;touch-action:none;
    -webkit-user-select:none;user-select:none}
  #stage{position:absolute;left:0;top:0;transform-origin:0 0;background:#fff;
    box-shadow:0 1px 8px rgba(0,0,0,.25)}
  #stage svg{display:block;width:100%;height:100%}
  #hint{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;
    font:15px sans-serif;color:#555;text-align:center;padding:24px}
</style></head>
<body>
<div id="hint"></div>
<div id="stage"></div>
<script>
(function(){
  var stage=document.getElementById('stage'), hint=document.getElementById('hint');
  var aspect=1.414, z=1, x=0, y=0, s=1;      // z: committed zoom, s: zoom during a pinch
  var MINZ=0.5, MAXZ=40;
  var hits=[], selected='', tapTimer=null, down=null;
  function baseW(){ return Math.min(innerWidth*0.96, innerHeight*0.96/aspect); }
  function layout(){ var w=baseW()*z; stage.style.width=w+'px'; stage.style.height=(w*aspect)+'px'; }
  function apply(){ stage.style.transform='translate('+x+'px,'+y+'px) scale('+s+')'; }
  // fitted: the sheet is fitted and the user has not moved it, so a WebView resize
  // (the first screen layout arrives after the drawing) fits it again.
  var fitted=false;
  function fit(){ fitted=true; z=1; s=1; layout(); var w=baseW();
    x=(innerWidth-w)/2; y=(innerHeight-w*aspect)/2; apply(); }

  function show(text, keep){
    stage.innerHTML=text;
    var svg=stage.querySelector('svg');
    if(!svg){ hint.textContent='Could not show the drawing'; hint.style.display='flex'; drawn(); return; }
    var vb=svg.viewBox && svg.viewBox.baseVal;
    if(vb && vb.width>0){ aspect=vb.height/vb.width; }
    svg.removeAttribute('width'); svg.removeAttribute('height');
    // Links in the drawing (draftwright's credit in the title block) must not leave the page.
    svg.querySelectorAll('a').forEach(function(a){
      a.removeAttribute('href'); a.removeAttributeNS('http://www.w3.org/1999/xlink','href'); });
    hint.style.display='none';
    if(keep){ layout(); apply(); } else { fit(); }
    mark();
    drawn();
  }
  // Two frames: the first lays the SVG out, the second has it on screen.
  function drawn(){ requestAnimationFrame(function(){ requestAnimationFrame(function(){
    window.bridge.postMessage('drawn'); }); }); }

  // --- picking a label by tap ---
  function svgPoint(cx, cy){
    var svg=stage.querySelector('svg'); if(!svg) return null;
    var vb=svg.viewBox.baseVal, r=svg.getBoundingClientRect();
    if(!vb || r.width<=0) return null;
    return {x:vb.x+(cx-r.left)/r.width*vb.width, y:vb.y+(cy-r.top)/r.height*vb.height,
            unit:vb.width/r.width};
  }
  function pick(cx, cy){
    var p=svgPoint(cx, cy); if(!p) return '';
    var pad=16*p.unit, best='', bestD=1e9, bestA=1e9;
    hits.forEach(function(h){
      var dx=Math.max(h[1]-p.x, 0, p.x-h[3]), dy=Math.max(h[2]-p.y, 0, p.y-h[4]);
      var d=Math.hypot(dx, dy), a=(h[3]-h[1])*(h[4]-h[2]);
      if(d>pad) return;
      if(d<bestD-1e-6 || (Math.abs(d-bestD)<1e-6 && a<bestA)){ best=h[0]; bestD=d; bestA=a; }
    });
    return best;
  }
  function mark(){
    var svg=stage.querySelector('svg'); if(!svg) return;
    var old=svg.querySelector('#dw-sel'); if(old) old.remove();
    var h=hits.filter(function(h){ return h[0]===selected; })[0]; if(!h) return;
    var m=0.8, r=document.createElementNS('http://www.w3.org/2000/svg','rect');
    r.setAttribute('id','dw-sel');
    r.setAttribute('x',h[1]-m); r.setAttribute('y',h[2]-m);
    r.setAttribute('width',h[3]-h[1]+2*m); r.setAttribute('height',h[4]-h[2]+2*m);
    r.setAttribute('rx',1); r.setAttribute('fill','rgba(47,91,234,0.18)');
    r.setAttribute('stroke','rgb(47,91,234)'); r.setAttribute('stroke-width',0.35);
    svg.appendChild(r);
  }

  window.bridge=window.bridge||{};
  window.bridge.onMessage=function(data){
    var i=data.indexOf(':'); if(i<0) return;
    var kind=data.slice(0,i);
    if(kind==='bg'){ document.body.style.background=data.slice(i+1); return; }
    if(kind==='hits'){ try{ hits=JSON.parse(data.slice(i+1)); }catch(e){ hits=[]; } mark(); return; }
    if(kind==='sel'){ selected=data.slice(i+1); mark(); return; }
    if(kind==='clear'){ stage.innerHTML=''; stage.style.width='0'; stage.style.height='0';
      hits=[]; selected=''; return; }
    show(data.slice(i+1), kind==='keep');
  };

  // --- gestures ---
  var start=null, lastTap=0;
  function dist(t){ var dx=t[0].clientX-t[1].clientX, dy=t[0].clientY-t[1].clientY; return Math.hypot(dx,dy); }
  function mid(t){ return {x:(t[0].clientX+t[1].clientX)/2, y:(t[0].clientY+t[1].clientY)/2}; }
  function commit(){
    if(s!==1){ var nz=Math.max(MINZ,Math.min(MAXZ,z*s)); s=nz/z; z=nz;
      // commit: element size ×s, transform back to 1
      s=1; layout(); apply(); }
  }
  document.addEventListener('touchstart',function(e){
    var t=e.touches;
    if(t.length===2){ start={kind:'pinch', d:dist(t), m:mid(t), x:x, y:y}; }
    else if(t.length===1){
      var now=Date.now();
      down={x:t[0].clientX, y:t[0].clientY, at:now};
      if(tapTimer){ clearTimeout(tapTimer); tapTimer=null; }
      if(now-lastTap<300){ fit(); start=null; lastTap=0; down=null; e.preventDefault(); return; }
      lastTap=now;
      start={kind:'pan', px:t[0].clientX, py:t[0].clientY, x:x, y:y};
    }
  },{passive:false});
  document.addEventListener('touchmove',function(e){
    e.preventDefault(); if(!start) return; var t=e.touches; fitted=false;
    if(start.kind==='pinch' && t.length===2){
      var r=dist(t)/start.d; r=Math.max(MINZ/z,Math.min(MAXZ/z,r)); var m=mid(t);
      s=r; x=m.x-(start.m.x-start.x)*r; y=m.y-(start.m.y-start.y)*r; apply();
    } else if(start.kind==='pan' && t.length===1){
      x=start.x+t[0].clientX-start.px; y=start.y+t[0].clientY-start.py; apply();
    }
  },{passive:false});
  document.addEventListener('touchend',function(e){
    if(start && start.kind==='pinch'){ commit(); down=null; }
    // A single tap: the finger barely moved. Wait to see whether a second tap follows.
    if(down && e.touches.length===0 && e.changedTouches.length===1){
      var c=e.changedTouches[0];
      if(Date.now()-down.at<400 && Math.hypot(c.clientX-down.x, c.clientY-down.y)<10){
        var cx=c.clientX, cy=c.clientY;
        tapTimer=setTimeout(function(){ tapTimer=null; var id=pick(cx, cy);
          selected=id; mark(); window.bridge.postMessage('tap:'+id); }, 280);
      }
    }
    down=null;
    start=null;
    if(e.touches.length===1){ var t=e.touches[0]; start={kind:'pan', px:t.clientX, py:t.clientY, x:x, y:y}; }
  });
  window.addEventListener('resize',function(){ if(fitted){ fit(); } else { layout(); apply(); } });

  window.bridge.postMessage('ready');
})();
</script>
</body></html>"##;
