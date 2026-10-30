import AppKit
import WebKit
// Native instrument for #1991 A1/A3/A4. Reuses the bundled page and #1932 fixture; IPC never leaves this process.
final class Probe:NSObject,WKNavigationDelegate {
 let web:WKWebView, window:NSWindow, root:String, out:String
 var index=0
 let cases=[("dark",18,800),("dark",20,800),("light",18,800),("light",20,800),("dark",20,350),("light",20,350)]
 init(_ root:String,_ out:String)throws {
  self.root=root;self.out=out
  let config=WKWebViewConfiguration();config.websiteDataStore = .nonPersistent()
  web=WKWebView(frame:NSRect(x:0,y:0,width:800,height:600),configuration:config)
  window=NSWindow(contentRect:NSRect(x:-20000,y:-20000,width:800,height:600),styleMask:[.borderless],backing:.buffered,defer:false)
  super.init();window.contentView=web;window.orderFront(nil);web.navigationDelegate=self
  try FileManager.default.createDirectory(atPath:out,withIntermediateDirectories:true)
  web.loadHTMLString(try String(contentsOfFile:root+"/build/web/workspace.html",encoding:.utf8),baseURL:nil)
  DispatchQueue.main.asyncAfter(deadline:.now()+50){fputs("native probe timeout\n",stderr);exit(1)}
 }
 func webView(_ webView:WKWebView,didFinish navigation:WKNavigation!){next()}
 func next(){
  if index==cases.count {NSApp.terminate(nil);return}
  let(theme,font,width)=cases[index],name="\(theme)-\(font)-\(width)"
  window.setContentSize(NSSize(width:width,height:600))
  let palette=theme=="dark" ? "--work:#38E8A0;--ask:#FFC247;--failed:#FF6378;--ready:#B9C6D8;--muted:#B9C6D8;--new:#6EA8FF;--review:#BCA3EB" : "--work:#087F5B;--ask:#995200;--failed:#C92641;--ready:#536171;--muted:#536171;--new:#1F65C5;--review:#8250DF"
  let fixture=try! String(contentsOfFile:root+"/tests/fixtures/conversation-shots.js",encoding:.utf8)
  let script="window.ipc={postMessage:t=>{(window.sent||(window.sent=[])).push(JSON.parse(t))}};"+fixture+";taskqConversationShot('question',{font:\(font),theme:'\(theme)',ai:{},palette:'\(palette)',host:'fixture',platform:'macos',version:'1991 probe'});"
  web.evaluateJavaScript(script){_,error in
   if let error=error {fputs("\(error)\n",stderr);exit(1)}
   self.web.callAsyncJavaScript("""
    while(!document.querySelector('[data-c-field=answer]')?.value.length)await new Promise(r=>setTimeout(r,25));
    await new Promise(r=>setTimeout(r,100));
    // Off-screen WKWebView suspends requestAnimationFrame; timers let the existing fixture finish.
    const visible=e=>{if(!e)return false;const r=e.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);return r.top>=0&&r.bottom<=innerHeight&&r.left>=0&&r.right<=innerWidth&&!!hit&&(hit===e||e.contains(hit))};
    const rect=e=>e&&e.getBoundingClientRect().toJSON();
    const input=document.querySelector('[data-c-field=answer]'),send=document.querySelector('[data-a=answer-send]'),attach=document.querySelector('[data-reply-files=pick]')?.parentElement,help=document.querySelector('.composer-help>summary');
    return JSON.stringify({width:innerWidth,height:innerHeight,font:getComputedStyle(input).fontSize,input:{rect:rect(input),client:input.clientHeight,scroll:input.scrollHeight,resize:getComputedStyle(input).resize,overflow:getComputedStyle(input).overflowY},send:{rect:rect(send),visible:visible(send)},attach:visible(attach),help:visible(help),reader:rect(document.querySelector('.conversation-reader')),editor:rect(document.querySelector('.answer-composer')),effects:(window.sent||[]).filter(m=>['answer','ai','conversation-action','reply-file'].includes(m.a))});
   """,arguments:[:],in:nil,in:.page){result in
    switch result {case .failure(let error):fputs("\(error)\n",stderr);exit(1)
    case .success(let value):
     let text=value as! String
     let facts=try! JSONSerialization.jsonObject(with:Data(text.utf8)) as! [String:Any]
     let input=facts["input"] as! [String:Any],send=facts["send"] as! [String:Any]
     guard send["visible"] as? Bool == true,facts["attach"] as? Bool == true,facts["help"] as? Bool == true,
           input["client"] as! Int >= input["scroll"] as! Int,input["resize"] as? String == "none",
           facts["font"] as? String == "\(font)px",(facts["effects"] as! [Any]).isEmpty else {
      fputs("FAIL \(name) \(text)\n",stderr);exit(1)
     }
     print("\(name) \(text)")
     try! text.write(toFile:self.out+"/"+name+".json",atomically:true,encoding:.utf8)
     let config=WKSnapshotConfiguration();config.rect=self.web.bounds
     self.web.takeSnapshot(with:config){image,error in
      guard let image=image,let tiff=image.tiffRepresentation,let bitmap=NSBitmapImageRep(data:tiff),let png=bitmap.representation(using:.png,properties:[:])else{fputs("snapshot failed \(String(describing:error))\n",stderr);exit(1)}
      try! png.write(to:URL(fileURLWithPath:self.out+"/"+name+".png"));self.index+=1;self.next()
     }
    }
   }
  }
 }
}
NSApplication.shared.setActivationPolicy(.accessory)
let probe=try Probe(CommandLine.arguments[1],CommandLine.arguments[2]);NSApp.run()
