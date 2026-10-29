//! Canonical six-bin Summary SVG (I1–I6 / R-UI4/R-UI5). Compact Q keeps its approved raster asset.

use serde::{Serialize, Deserialize};
use std::fmt::Write;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bin {
    pub start: f64,
    pub end: f64,
    pub counts: [u32; 4],
    pub missing: Vec<String>,
    pub ongoing: bool,
}
impl Bin { pub fn complete(&self) -> bool { self.missing.is_empty() } }

pub struct Data {
    pub counts: [Option<usize>; 4],
    pub blocked: Option<usize>,
    pub bins: Vec<Bin>,
    pub paused: bool,
}

/// An RGBA image, `s` pixels per point of the 22 pt reference drawing.
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub s: f32,
    pub px: Vec<u8>,
}

impl Rgba {
    pub fn new(w: u32, h: u32, s: f32) -> Rgba {
        Rgba { w, h, s, px: vec![0; (w * h * 4) as usize] }
    }

    /// Paint `rgb` over pixel `o` with coverage `cov` (0..=1); alpha 0 clears.
    fn blend(&mut self, x: u32, y: u32, rgb: [u8; 4], cov: f32) {
        let o = ((y * self.w + x) * 4) as usize;
        let da = self.px[o + 3] as f32 / 255.0;
        if rgb[3] == 0 {
            self.px[o + 3] = (da * (1.0 - cov) * 255.0) as u8;
            return;
        }
        let a = cov * rgb[3] as f32 / 255.0;
        let oa = a + da * (1.0 - a);
        for c in 0..3 {
            self.px[o + c] = ((rgb[c] as f32 * a + self.px[o + c] as f32 * da * (1.0 - a)) / oa) as u8;
        }
        self.px[o + 3] = (oa * 255.0) as u8;
    }

    /// Paint `rgb` where `inside(x, y)` holds, in points, 4×4 samples per pixel.
    pub fn fill(&mut self, rgb: [u8; 4], inside: impl Fn(f32, f32) -> bool) {
        self.fill_px(rgb, 0..self.w, 0..self.h, inside);
    }

    /// `fill` of `rect(x0, y0, x1, y1)`, visiting only the pixels it can cover.
    fn fill_rect(&mut self, rgb: [u8; 4], x0: f32, y0: f32, x1: f32, y1: f32) {
        let span = |a: f32, b: f32, n: u32| ((a * self.s).floor().max(0.0) as u32).min(n)..((b * self.s).ceil().max(0.0) as u32).min(n);
        let (xs, ys) = (span(x0, x1, self.w), span(y0, y1, self.h));
        self.fill_px(rgb, xs, ys, rect(x0, y0, x1, y1));
    }

    fn fill_px(&mut self, rgb: [u8; 4], xs: std::ops::Range<u32>, ys: std::ops::Range<u32>, inside: impl Fn(f32, f32) -> bool) {
        for y in ys {
            for x in xs.clone() {
                let hit = (0..16).filter(|i| inside((x as f32 + (i % 4) as f32 / 4.0 + 0.125) / self.s, (y as f32 + (i / 4) as f32 / 4.0 + 0.125) / self.s)).count();
                if hit > 0 {
                    self.blend(x, y, rgb, hit as f32 / 16.0);
                }
            }
        }
    }

    pub fn png(&self) -> Vec<u8> {
        let mut out = vec![];
        let mut e = ::png::Encoder::new(&mut out, self.w, self.h);
        e.set_color(::png::ColorType::Rgba);
        if let Ok(mut w) = e.write_header() {
            let _ = w.write_image_data(&self.px);
        }
        out
    }
}

pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> impl Fn(f32, f32) -> bool {
    move |x, y| x >= x0 && x < x1 && y >= y0 && y < y1
}

fn rrect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> impl Fn(f32, f32) -> bool {
    move |x, y| {
        let (dx, dy) = ((x0 + r - x).max(x - x1 + r).max(0.0), (y0 + r - y).max(y - y1 + r).max(0.0));
        x >= x0 && x < x1 && y >= y0 && y < y1 && dx * dx + dy * dy <= r * r
    }
}

pub struct Palette {
    pub ink: [u8;4], pub work: [u8;4], pub ask: [u8;4], pub muted: [u8;4], pub failed: [u8;4], pub review: [u8;4],
}
pub fn palette(dark: bool) -> Palette {
    if dark { Palette { ink:[236,239,242,255], work:[56,232,160,255], ask:[255,194,71,255], muted:[185,198,216,255], failed:[255,99,120,255], review:[188,163,235,255] } }
    else { Palette { ink:[37,41,48,255], work:[8,127,91,255], ask:[153,82,0,255], muted:[83,97,113,255], failed:[201,38,65,255], review:[130,80,223,255] } }
}
fn hex(c:[u8;4])->String { format!("#{:02X}{:02X}{:02X}",c[0],c[1],c[2]) }
/// The same semantic tokens supply menu counters and chart colors; no CSS color table.
pub fn css(dark:bool)->String {
    let p=palette(dark);
    format!("--work:{};--ask:{};--failed:{};--ready:{};--muted:{};--review:{}",hex(p.work),hex(p.ask),hex(p.failed),hex(p.muted),hex(p.muted),hex(p.review))
}

/// The angular q (18·z pt, stroke 1.8·z, tail bottom right) at `(x0, y0)` with ▶ (admits) or Ⅱ inside.
fn draw_q(img: &mut Rgba, x0: f32, y0: f32, z: f32, run: bool, ink: [u8; 4], p: &Palette) {
    let (x1, y1, t) = (x0 + 18.0 * z, y0 + 18.0 * z, 1.8 * z);
    let (outer, inner) = (rrect(x0, y0, x1, y1, t), rrect(x0 + t, y0 + t, x1 - t, y1 - t, 0.4 * z));
    img.fill(ink, |x, y| outer(x, y) && !inner(x, y));
    img.fill_rect(ink, x1 - t, y1 - z, x1, y1 + 3.6 * z);
    let (cx, cy, h) = (x0 + 9.0 * z, y0 + 9.0 * z, 4.5 * z);
    if run {
        let (l, t) = (cx - 3.15 * z, cy - h);
        img.fill(p.work, move |x, y| x >= l && y >= t + (x - l) * 4.5 / 8.1 && y <= t + 2.0 * h - (x - l) * 4.5 / 8.1);
    } else {
        img.fill_rect(p.muted, cx - h, cy - h, cx - 0.4 * h, cy + h);
        img.fill_rect(p.muted, cx + 0.4 * h, cy - h, cx + h, cy + h);
    }
}

fn compact_palette(dark:bool)->Palette {
    // Compact identity and shipped app resources remain byte-identical.
    let mut p=palette(dark);
    p.work=if dark {[119,205,179,255]}else{[35,120,98,255]};
    p.muted=if dark {[180,189,200,255]}else{[98,107,119,255]};
    p
}

/// The Icon view: the compact q in a 22 pt square at `s` pixels per point.
#[tracing::instrument(name="icon.compact",skip_all)]
pub fn compact(run: bool, dark: bool, s: f32) -> Rgba {
    let p=compact_palette(dark);
    let n = (22.0 * s).round() as u32;
    let mut img = Rgba::new(n, n, s);
    // Center the body; leave space below for the tail without shifting the body up.
    let z = 22.0 / 25.2;
    let inset = (22.0 - 18.0 * z) / 2.0;
    draw_q(&mut img, inset, inset, z, run, p.ink, &p);
    img
}


pub fn peak(bins:&[Bin])->u32 {
    bins.iter().filter(|b|b.complete()).map(|b|b.counts[0]).max().unwrap_or(0)
}

/// One source for every surface. History changes the viewport and reveals Ask marks, preserving the shared vector geometry.
#[tracing::instrument(name="icon.svg",skip_all)]
pub fn svg(d:&Data,dark:bool)->String {
    let p=palette(dark);
    let color=|c|hex(if d.paused {p.muted}else{c});
    let peak=peak(&d.bins);
    let mut counters=Vec::new();
    let mut end=0.0f64;
    for (i,(value,c)) in [(d.counts[0],p.work),(d.counts[1],p.ask),(d.counts[2].zip(d.blocked).map(|(a,b)|a.saturating_add(b)),p.muted)].into_iter().enumerate() {
        if value==Some(0) {continue;}
        let text=value.map_or("–".into(),|n|if n>99 {"99+".into()}else{n.to_string()});
        let (paths,width)=counter(&text,color(c));
        let x=if counters.is_empty() {[1.0f64,8.1,15.4][i]}else{[1.0f64,8.1,15.4][i].max(end+1.5)};
        end=x+width;
        counters.push((i,paths,width,x));
    }
    let ink=counters.iter().map(|c|c.2).sum::<f64>()+1.5*counters.len().saturating_sub(1) as f64;
    let width=22.0f64.max(ink+2.0);
    let mut right=width-1.0;
    for (_,_,w,x) in counters.iter_mut().rev() {
        *x=x.min(right-*w);right=*x-1.5;
    }
    let mut out=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} 22">"#);
    if d.paused {
        out.push_str(&format!(r#"<defs><mask id="pause-clear"><rect width="{width}" height="22" fill="white"/><rect x="3.5" y="3.5" width="15" height="17.7" fill="black"/></mask></defs><g mask="url(#pause-clear)">"#));
    }
    let empty=d.counts.iter().all(|n|*n==Some(0))&&d.blocked==Some(0)&&d.bins.iter().all(|b|b.complete()&&b.counts==[0;4]);
    write!(out,r#"<g id="summary-data" visibility="{}"><g id="chart" shape-rendering="crispEdges">"#,if empty {"hidden"}else{"visible"}).unwrap();
    for (i,b) in d.bins.iter().enumerate() {
        let x=1.0+3.5*i as f64;
        write!(out,r#"<g id="bin-{i}" data-start="{}" data-end="{}">"#,b.start,b.end).unwrap();
        let (y,h,c,mark)=if !b.complete() {(15.5,0.75,p.muted,"unknown")}
            else if b.counts[0]==0 {(20.0,0.5,p.muted,"zero")}
            else {let h=9.0*b.counts[0] as f64/peak.max(1) as f64;(20.5-h,h,p.work,"closed")};
        write!(out,r#"<rect data-mark="{mark}" x="{x}" y="{y}" width="2.5" height="{h}" fill="{}"/>"#,color(c)).unwrap();
        if b.counts[2]>0 {write!(out,r#"<rect data-mark="ask" visibility="hidden" x="{x}" y="12" width="2.5" height="0.75" fill="{}"/>"#,color(p.ask)).unwrap();}
        if b.counts[1]>0 {write!(out,r#"<rect data-mark="failed" x="{x}" y="10.5" width="2.5" height="0.75" fill="{}"/>"#,color(p.failed)).unwrap();}
        if b.ongoing {write!(out,r#"<rect data-mark="ongoing" x="{x}" y="21" width="2.5" height="0.5" fill="{}"/>"#,color(p.muted)).unwrap();}
        out.push_str("</g>");
    }
    out.push_str(r#"</g><g id="counters">"#);
    for (i,paths,_,x) in counters {
        write!(out,r#"<g id="counter-{i}" transform="translate({x} 0)">{paths}</g>"#).unwrap();
    }
    out.push_str("</g>");
    out.push_str("</g>");
    if empty&&!d.paused {let z=22.0/25.2;let inset=(22.0-18.0*z)/2.0;write!(out,r#"<g id="empty-q">{}</g>"#,q_svg(true,&compact_palette(dark),inset,inset,z)).unwrap();}
    // I4: gray data under the existing centered compact Q/pause overlay.
    if d.paused {out.push_str("</g>");let gray=Palette {ink:p.muted,..p};out.push_str(&q_svg(false,&gray,4.25,4.25,0.75));}
    out.push_str("</svg>");out
}

/// Vector form of the existing compact Q geometry; used only for empty fallback and the pause overlay.
fn q_svg(run:bool,p:&Palette,x:f64,y:f64,z:f64)->String {
    let c=hex(p.ink);let state=hex(if run {p.work}else{p.muted});
    let (r,t)=(18.0,1.8);
    let body=format!("M{t} 0H{}A{t} {t} 0 0 1 {r} {t}V{}A{t} {t} 0 0 1 {} {r}H{t}A{t} {t} 0 0 1 0 {}V{t}A{t} {t} 0 0 1 {t} 0Z M2.2 1.8H15.8A.4 .4 0 0 1 16.2 2.2V15.8A.4 .4 0 0 1 15.8 16.2H2.2A.4 .4 0 0 1 1.8 15.8V2.2A.4 .4 0 0 1 2.2 1.8Z",r-t,r-t,r-t,r-t);
    let mark=if run {"M5.85 4.5L13.95 9L5.85 13.5Z"}else{"M4.5 4.5H7.2V13.5H4.5ZM10.8 4.5H13.5V13.5H10.8Z"};
    format!(r#"<g transform="translate({x} {y}) scale({z})"><path fill="{c}" fill-rule="evenodd" d="{body}"/><rect x="16.2" y="17" width="1.8" height="4.6" fill="{c}"/><path fill="{state}" d="{mark}"/></g>"#)
}

/// Rasterize the final SVG directly at device scale; resvg uses premultiplied RGBA.
pub fn raster(svg:&str,n:u32)->Result<Rgba,String> {
    raster_size(svg,n,false)
}

/// Menu-bar height stays fixed while I2 may expand its horizontal slot.
pub fn raster_slot(svg:&str,n:u32)->Result<Rgba,String> {
    raster_size(svg,n,true)
}

fn raster_size(svg:&str,n:u32,slot:bool)->Result<Rgba,String> {
    let tree=resvg::usvg::Tree::from_str(svg,&resvg::usvg::Options::default()).map_err(|e|e.to_string())?;
    let scale=n as f32/if slot {22.0}else{tree.size().width().max(tree.size().height())};
    let w=if slot {(tree.size().width()*scale).ceil() as u32}else{n};
    let mut pix=resvg::tiny_skia::Pixmap::new(w,n).ok_or("invalid icon dimensions")?;
    let transform=resvg::tiny_skia::Transform::from_scale(scale,scale).post_translate(0.0,(n as f32-tree.size().height()*scale)/2.0);
    resvg::render(&tree,transform,&mut pix.as_mut());
    let mut img=Rgba::new(w,n,scale);
    for (to,from) in img.px.chunks_exact_mut(4).zip(pix.data().chunks_exact(4)) {
        let a=from[3] as u32;to[3]=from[3];
        if a>0 {for c in 0..3 {to[c]=((from[c] as u32*255+a/2)/a).min(255) as u8;}}
    }
    Ok(img)
}

/// SF outlines are shared as paths, so WebKit and resvg paint identical glyph ink.
#[cfg(target_os="macos")]
fn counter(text:&str,color:String)->(String,f64) {
    use objc2_app_kit::{NSFont,NSFontWeightMedium};
    use objc2_core_foundation::{CGPoint,CGSize,CGRect};
    use objc2_core_text::{CTFont,CTFontOrientation};
    use std::{ffi::c_void,ptr::NonNull};
    #[repr(C)] struct Element {kind:i32,points:*const CGPoint}
    unsafe extern "C" {
        fn CTFontCreatePathForGlyph(font:*const CTFont,glyph:u16,matrix:*const c_void)->*const c_void;
        fn CGPathApply(path:*const c_void,info:*mut c_void,callback:unsafe extern "C" fn(*mut c_void,*const Element));
        fn CGPathGetPathBoundingBox(path:*const c_void)->CGRect;
        fn CGPathRelease(path:*const c_void);
    }
    unsafe extern "C" fn append(info:*mut c_void,element:*const Element) {
        // SAFETY: synchronous CGPathApply, valid callback pointers and stack-owned String.
        unsafe {
            let out=&mut *(info as *mut String);let e=&*element;
            let (op,n)=match e.kind {0=>("M",1),1=>("L",1),2=>("Q",2),3=>("C",3),_=>("Z",0)};
            out.push_str(op);
            for i in 0..n {let p=*e.points.add(i);let _=write!(out,"{} {} ",p.x,p.y);}
        }
    }
    // SAFETY: framework constant, toll-free NSFont/CTFont bridge and length-checked buffers.
    unsafe {
        let font=NSFont::systemFontOfSize_weight(9.0,NSFontWeightMedium);
        let ct=&*(&*font as *const NSFont as *const CTFont);
        let outline=|s:&str| {
            let mut chars:Vec<u16>=s.encode_utf16().collect();let n=chars.len();
            let mut glyphs=vec![0u16;n];let mut advances=vec![CGSize::new(0.0,0.0);n];
            ct.glyphs_for_characters(NonNull::new_unchecked(chars.as_mut_ptr()),NonNull::new_unchecked(glyphs.as_mut_ptr()),n as isize);
            ct.advances_for_glyphs(CTFontOrientation::Horizontal,NonNull::new_unchecked(glyphs.as_mut_ptr()),advances.as_mut_ptr(),n as isize);
            let mut paths=String::new();let(mut pen,mut left,mut right)=(0.0, f64::INFINITY, f64::NEG_INFINITY);
            for (g,a) in glyphs.iter().zip(advances) {
                let path=CTFontCreatePathForGlyph(ct,*g,std::ptr::null());
                if !path.is_null() {
                    let b=CGPathGetPathBoundingBox(path);left=left.min(pen+b.origin.x);right=right.max(pen+b.origin.x+b.size.width);
                    let mut d=String::new();CGPathApply(path,(&mut d as *mut String).cast(),append);CGPathRelease(path);
                    write!(paths,r#"<path transform="translate({pen} 0)" d="{d}"/>"#).unwrap();
                }
                pen+=a.width;
            }
            (paths,left,right-left)
        };
        let(paths,left,width)=outline(text);
        (format!(r#"<g fill="{color}" transform="translate({} 9) scale(1 -1)">{paths}</g>"#,-left),width)
    }
}

#[cfg(not(target_os="macos"))]
fn counter(text:&str,color:String)->(String,f64) {
    use resvg::usvg;
    static OPTIONS:std::sync::OnceLock<usvg::Options<'static>>=std::sync::OnceLock::new();
    let options=OPTIONS.get_or_init(||{let mut o=usvg::Options::default();o.fontdb_mut().load_system_fonts();o.fontdb_mut().set_sans_serif_family("Segoe UI");o});
    let layout=|s:&str|usvg::Tree::from_str(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22"><text id="value" y="9" font-family="Segoe UI" font-size="9" font-weight="500" fill="{color}">{s}</text></svg>"#),options).expect("fixed numeric SVG");
    let tree=layout(text);
    // Text bounds include side bearings; align the paths that are actually painted.
    let ink=|t:&usvg::Tree|t.node_by_id("value").and_then(|n|match n {usvg::Node::Text(t)=>Some(t.flattened().bounding_box()),_=>None});
    let (left,width)=ink(&tree).map_or((0.0,0.0),|b|(b.x(),b.width()));
    let svg=tree.to_string(&usvg::WriteOptions::default());
    (format!(r#"<g transform="translate({} 0)">{}</g>"#,-left as f64,&svg[svg.find('>').unwrap()+1..svg.rfind("</svg>").unwrap()]),width as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    // #1347: the tail must not pull the square body above the canvas center.
    #[test]
    fn q_body_is_centered() {
        for size in [16, 22, 44, 256] {
            for run in [true, false] {
                let image = compact(run, false, size as f32 / 22.0);
                let mid = size / 2;
                let visible = |x: u32, y: u32| image.px[((y * size + x) * 4 + 3) as usize] > 0;
                let xs: Vec<_> = (0..size).filter(|&x| visible(x, mid)).collect();
                let ys: Vec<_> = (0..size).filter(|&y| visible(mid, y)).collect();
                assert_eq!(xs.first(), ys.first(), "body top/left at {size}");
                assert_eq!(xs.last(), ys.last(), "body bottom/right at {size}");
                assert_eq!(xs[0] + xs[xs.len()-1], size - 1, "center at {size}");
            }
        }
    }

    // #1347: shipped resources must stay byte-identical to the existing compact Q renderer.
    // Regenerate deliberately with TASKQ_WRITE_APP_ICONS=1 cargo test app_icon_assets.
    #[test]
    fn app_icon_assets() {
        let mut chunks=Vec::new();
        for (kind,size) in [(b"icp4",16u32),(b"icp5",32),(b"icp6",64),(b"ic07",128),(b"ic08",256),(b"ic09",512),(b"ic10",1024)] {
            let png=compact(true,false,size as f32/22.0).png();
            chunks.extend_from_slice(kind);
            chunks.extend_from_slice(&((png.len()+8) as u32).to_be_bytes());
            chunks.extend(png);
        }
        let mut icns=b"icns".to_vec();
        icns.extend_from_slice(&((chunks.len()+8) as u32).to_be_bytes());icns.extend(chunks);
        let sizes=[16u32,24,32,48,64,128,256];
        let mut ico=vec![0,0,1,0,sizes.len() as u8,0];
        let mut images=Vec::new();
        for size in sizes {
            let png=compact(true,false,size as f32/22.0).png();
            ico.extend_from_slice(&[size as u8,size as u8,0,0,1,0,32,0]);
            ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
            ico.extend_from_slice(&((6+16*sizes.len()+images.len()) as u32).to_le_bytes());
            images.extend(png);
        }
        ico.extend(images);
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (name,bytes) in [("TaskQ.icns",icns),("TaskQ.ico",ico)] {
            let path=root.join(name);
            if std::env::var_os("TASKQ_WRITE_APP_ICONS").is_some() {
                std::fs::create_dir_all(&root).unwrap();std::fs::write(&path,&bytes).unwrap();
            }
            assert_eq!(std::fs::read(&path).expect("generated app icon asset"),bytes,"{name}: regenerate after changing Q");
        }
    }

    // Chain: native tray's first icon -> always-on collector -> committed readback.
    // Risk: per-sample instrumentation stalls the UI and loses observations despite renderer-only tests passing.
    #[test]
    fn first_icon_with_capture_finishes_without_collection_loss() {
        use std::process::{Command, Stdio};
        let root = std::env::temp_dir().join(format!("qi-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for mode in ["baseline", "capture", "expired"] {
            let log = root.join(format!("{mode}.log"));
            let output = std::fs::File::create(&log).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "icon::tests::first_icon_capture_child", "--ignored", "--nocapture"])
                .env("TASKQ_ICON_PROBE", &root).env("TASKQ_ICON_PROBE_MODE", mode)
                .stdout(Stdio::from(output.try_clone().unwrap())).stderr(Stdio::from(output)).spawn().unwrap();
            let began = std::time::Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() { break Some(status); }
                if began.elapsed() > std::time::Duration::from_secs(20) {
                    child.kill().unwrap(); child.wait().unwrap(); break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            };
            let detail = std::fs::read_to_string(&log).unwrap();
            eprintln!("{mode}: {detail}");
            assert!(status.is_some_and(|s| s.success()), "{mode}: icon/flush failed or exceeded20s; evidence {}", log.display());
        }
        assert_eq!(std::fs::read(root.join("baseline.png")).unwrap(), std::fs::read(root.join("capture.png")).unwrap());
        assert_eq!(std::fs::read(root.join("baseline.png")).unwrap(), std::fs::read(root.join("expired.png")).unwrap());
        if !cfg!(debug_assertions) {
            let p95 = |mode: &str| {
                let mut values: Vec<u64> = serde_json::from_slice(&std::fs::read(root.join(format!("{mode}.json"))).unwrap()).unwrap();
                assert_eq!(values.len(), 30); values.sort_unstable(); values[28]
            };
            let baseline = p95("baseline");
            for mode in ["capture", "expired"] {
                let captured = p95(mode);
                eprintln!("30 renders p95: baseline={baseline}us {mode}={captured}us");
                assert!(captured <= baseline + 1000.max(baseline / 20), "{mode} capture exceeded the operation budget; evidence {}", root.display());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "subprocess helper exercised by first_icon_with_capture_finishes_without_collection_loss"]
    fn first_icon_capture_child() {
        let root = std::path::PathBuf::from(std::env::var_os("TASKQ_ICON_PROBE").unwrap());
        let mode = std::env::var("TASKQ_ICON_PROBE_MODE").unwrap();
        let db = root.join(format!("{mode}.db"));
        let runtime = (mode != "baseline").then(|| coreq::telemetry::capture::Runtime::install(&db, "taskq", "fixture").unwrap());
        if mode == "expired" {
            coreq::telemetry::capture::diagnose("unused", std::time::Duration::from_nanos(1), 1).unwrap();
            assert_eq!(coreq::telemetry::capture::qualify().unwrap()["diagnostics"]["state"], "expired");
        }
        let mut timings = Vec::new();
        for n in 0..30 {
            let began = std::time::Instant::now();
            let image = compact(false, true, 2.0);
            timings.push(began.elapsed().as_micros() as u64);
            if n == 0 {
                eprintln!("first compact icon: {}us", timings[0]);
                std::fs::write(root.join(format!("{mode}.png")), image.png()).unwrap();
            }
        }
        std::fs::write(root.join(format!("{mode}.json")), serde_json::to_vec(&timings).unwrap()).unwrap();
        if runtime.is_some() {
            eprintln!("capture: {}", coreq::telemetry::capture::qualify().unwrap());
            let reader = coreq::telemetry::Reader::open(&db).unwrap();
            let exported = reader.export(0, 0, 1024).unwrap();
            let rows = exported["events"].as_array().unwrap();
            assert_eq!(rows.iter().filter(|r| r["observation"]["operation"] == "taskq.icon.compact" && r["observation"]["kind"] == "method").count(), 30);
            assert!(!rows.iter().any(|r| r["observation"]["source"] == "diagnostic"), "normal rendering generated internal detail");
        }
    }

    // #1725 A2–A4: History can reveal Ask without changing icon pixels, Closed scale or coverage marks.
    #[test]
    fn history_ask_marks_preserve_closed_zero_unknown_and_icon_pixels() {
        let bins = [0, 0, 1, 2, 999, 0].into_iter().enumerate().map(|(i, closed)| Bin {
            start: i as f64, end: (i + 1) as f64, counts: [closed, u32::from(i == 2), [3, 0, 1, 0, 2, 0][i], 0],
            missing: if i == 4 { vec!["synthetic missing source".into()] } else { vec![] }, ongoing: i == 5,
        }).collect();
        let mut data = Data { counts: [Some(1); 4], blocked: Some(0), bins, paused: false };
        assert_eq!(peak(&data.bins), 2);
        for dark in [false, true] {
            let vector = svg(&data, dark);
            let bin = |i| vector.split(&format!(r#"<g id="bin-{i}" "#)).nth(1).unwrap().split("</g>").next().unwrap();
            assert_eq!(vector.matches(r#"data-mark="ask""#).count(), 3, "ask-only, mixed and observed incomplete Ask");
            for i in [0, 2, 4] {
                assert!(bin(i).contains(r#"data-mark="ask" visibility="hidden""#), "icons hide History-only Ask");
                assert!(bin(i).contains(&format!(r#"y="12" width="2.5" height="0.75" fill="{}""#, hex(palette(dark).ask))));
            }
            for (i, mark) in [(0, "zero"), (1, "zero"), (2, "closed"), (2, "failed"), (4, "unknown"), (5, "ongoing")] {
                assert!(bin(i).contains(&format!(r#"data-mark="{mark}""#)));
            }
            assert!(bin(2).contains(r#"y="16" width="2.5" height="4.5""#), "Closed 1 / peak 2");
            let asks: Vec<_> = data.bins.iter().map(|b| b.counts[2]).collect();
            for b in &mut data.bins { b.counts[2] = 0; }
            assert_eq!(raster(&vector, 256).unwrap().px, raster(&svg(&data, dark), 256).unwrap().px, "menu-bar/Dock keep closed-only pixels");
            for (b, ask) in data.bins.iter_mut().zip(asks) { b.counts[2] = ask; }
        }
    }

    // #1789 A1/A2: exact labels and measured ink must survive every position without collisions or clipping.
    #[test]
    fn exact_two_digit_counters_reclaim_space_before_widening() {
        let bins=(0..6).map(|i|Bin {start:i as f64,end:(i+1) as f64,counts:[1,0,0,0],missing:vec![],ongoing:i==5}).collect();
        let mut data=Data {counts:[Some(0);4],blocked:Some(0),bins,paused:false};
        for dark in [false,true] {
            for n in [0,9,10,12,99,100] {
                for mask in 1u32..8 {
                    data.counts=[Some(0);4];
                    for i in 0..3 {if mask&(1<<i)!=0 {data.counts[i]=Some(n);}}
                    let vector=svg(&data,dark);
                    let tree=resvg::usvg::Tree::from_str(&vector,&resvg::usvg::Options::default()).unwrap();
                    let mut right=0.0;
                    for i in 0..3 {
                        let node=tree.node_by_id(&format!("counter-{i}"));
                        if data.counts[i]==Some(0) {assert!(node.is_none());continue;}
                        let bounds=node.unwrap().abs_bounding_box();
                        if n>=10 {assert!(bounds.width()>7.0,"{n} must retain multiple glyphs, not an overflow plus: {bounds:?}");}
                        assert!(bounds.x()>=right && bounds.right()<=tree.size().width()-0.99,"separate in-bounds ink: {vector}");
                        assert!(bounds.bottom()<10.5,"counter cannot touch the chart");
                        right=bounds.right()+1.49;
                    }
                    if n==100 {
                        for value in &mut data.counts[..3] {if *value!=Some(0) {*value=Some(usize::MAX);}}
                        assert_eq!(svg(&data,dark),vector,"100 and larger counts share the 99+ cap");
                    }
                    if mask.count_ones()==1 {assert_eq!(tree.size().width(),22.0,"hidden zero cells absorb two digits and 99+");}
                    for size in [22,44,256] {
                        let image=raster(&vector,size).unwrap();assert_eq!((image.w,image.h),(size,size));
                        let slot=raster_slot(&vector,size).unwrap();assert_eq!(slot.h,size);assert_eq!(slot.w,(tree.size().width()*size as f32/22.0).ceil() as u32);
                    }
                }
            }
            data.counts=[Some(100);4];data.paused=true;
            let paused=raster_slot(&svg(&data,dark),44).unwrap();
            assert!((0..20).any(|y|(44..paused.w).any(|x|paused.px[((y*paused.w+x)*4+3) as usize]>0)),"pause mask must retain the extended counters");
            data.paused=false;
        }
    }

    // A2/A3/A4/A5: SVG semantics survive every viewport; other events cannot alter closed height.
    #[test]
    fn six_columns_keep_quantity_marks_palette_and_device_bounds() {
        let bins:Vec<Bin>=[4,0,999,12,7,4].into_iter().enumerate().map(|(i,c)|Bin {start:i as f64,end:(i+1) as f64,counts:[c,0,0,0],missing:if i==2 {vec!["synthetic missing source".into()]}else{vec![]},ongoing:i==5}).collect();
        let mut data=Data {counts:[Some(0);4],blocked:Some(0),bins,paused:false};
        assert!(raster("",44).is_err(),"missing published SVG stays a render error");
        for dark in [false,true] {
            let svg=svg(&data,dark);
            assert_eq!(svg.matches("data-start=").count(),6);
            assert!(svg.contains(r#"y="17.5" width="2.5" height="3""#),"4 / peak 12");
            assert_eq!(svg.matches(r#"data-mark="unknown""#).count(),1);
            assert_eq!(svg.matches(r#"data-mark="zero""#).count(),1);
            assert_eq!(svg.matches(r#"data-mark="ongoing""#).count(),1);
            assert!(svg.contains(&hex(palette(dark).work)));
            for n in [16,18,22,33,44,66,256] {let image=raster(&svg,n).unwrap();assert_eq!((image.w,image.h),(n,n));assert!(image.px.chunks_exact(4).any(|p|p[3]>0));}
        }
        assert_eq!(peak(&data.bins),12,"incomplete 999 cannot set the scale");
        for (dark,expected) in [(false,[[8,127,91,255],[153,82,0,255],[201,38,65,255],[83,97,113,255],[130,80,223,255]]),(true,[[56,232,160,255],[255,194,71,255],[255,99,120,255],[185,198,216,255],[188,163,235,255]])] {
            let p=palette(dark);assert_eq!([p.work,p.ask,p.failed,p.muted,p.review],expected,"approved palette A");
        }
        assert!(css(false).ends_with(";--review:#8250DF")&&css(true).ends_with(";--review:#BCA3EB"),"Review accent token");
        let before=svg(&data,false);data.bins[0].counts=[4,8,7,6];
        let after=svg(&data,false);
        assert!(after.contains(r#"data-mark="failed""#));
        assert_eq!(before.matches(r#"y="17.5" width="2.5" height="3""#).count(),after.matches(r#"y="17.5" width="2.5" height="3""#).count());
        for b in &mut data.bins {b.counts=[0;4];b.missing.clear();}
        let zero=svg(&data,false);assert_eq!(zero.matches(r#"data-mark="zero""#).count(),6);assert!(!zero.contains("NaN"));
        data.counts=[None,Some(9),Some(1),Some(0)];
        let vector=svg(&data,false);let tree=resvg::usvg::Tree::from_str(&vector,&resvg::usvg::Options::default()).unwrap();
        for (i,left) in [1.0,8.1,15.4].into_iter().enumerate() {let bounds=tree.node_by_id(&format!("counter-{i}")).unwrap().abs_bounding_box();assert!((bounds.x()-left).abs()<0.01&&bounds.width()<=5.61,"ink alignment and overflow at cell {i}: {bounds:?}");}
        data.counts=[Some(0);4];data.blocked=Some(3);let blocked=svg(&data,false);
        data.counts[2]=Some(3);data.blocked=Some(0);assert_eq!(svg(&data,false),blocked,"ready + blocked retains meaning");
        data.counts=[Some(0);4];assert!(svg(&data,false).contains(r#"id="empty-q""#));
        data.paused=true;
        let paused=svg(&data,false);let p=palette(false);
        assert!(paused.contains("pause-clear")&&paused.contains(&hex(p.muted)));
        for c in [p.work,p.ask,p.failed] {assert!(!paused.contains(&hex(c)),"paused data stays gray");}

    }
}
