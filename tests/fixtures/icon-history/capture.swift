import AppKit
import WebKit

// Isolated native WebKit instrument: frozen Rust-generated data and the shipped bundled page. No board or service.
final class Capture: NSObject, WKNavigationDelegate {
    let web: WKWebView
    let window: NSWindow
    let root: String
    let output: String
    var inputs: [(String, [String:Any])] = []
    var index = 0
    init(root:String,output:String) throws {
        self.root=root; self.output=output
        let config=WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        web=WKWebView(frame:NSRect(x:0,y:0,width:360,height:800),configuration:config)
        window=NSWindow(contentRect:NSRect(x:-20000,y:-20000,width:360,height:800),styleMask:[.borderless],backing:.buffered,defer:false)
        super.init()
        window.contentView=web;window.orderBack(nil);web.navigationDelegate=self
        try FileManager.default.createDirectory(atPath:output,withIntermediateDirectories:true)
        for theme in ["light","dark"] {
            for (name,page) in [("day","cadence"),("hour","cadence"),("incomplete","cadence"),("day","tray-icon")] {
                let data=try Data(contentsOf:URL(fileURLWithPath:root+"/tests/fixtures/icon-history/"+name+"-"+theme+".json"))
                let history=try JSONSerialization.jsonObject(with:data) as! [String:Any]
                let s:[String:Any] = ["page":page,"font":16,"size":[360,800],"size_anchor":"fixed","theme":theme,"period":name=="hour" ? "1h" : "1d","host":"Frozen capture","version":"1598 candidate","platform":"macos","now":history["captured"]!,"palette":history["palette"]!,"view":"full","history":history,"icons":["full":history["svg"]!,"icon":history["compact"]!],"projects":[["root":"frozen","name":"Frozen TaskQ capture","tasks":[],"focus":[],"labels":[],"alive":true]],"ai":["draft":""]]
                inputs.append((name+"-"+theme+"-"+page,s))
            }
        }
        web.loadHTMLString(try String(contentsOfFile:root+"/build/web/tray.html",encoding:.utf8),baseURL:URL(fileURLWithPath:root+"/"))
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { next() }
    func next() {
        if index==inputs.count {assert(index==8);NSApp.terminate(nil);return}
        let (name,state)=inputs[index]
        do {
            let json=String(data:try JSONSerialization.data(withJSONObject:state),encoding:.utf8)!
            web.evaluateJavaScript("window.taskq.receive("+json+")") {_,error in
                if let error=error {fputs("native JS failed: \(error)\n",stderr);exit(1)}
                DispatchQueue.main.async {self.capture(name)}
            }
        } catch {fputs("native state failed: \(error)\n",stderr);exit(1)}
    }
    func capture(_ name:String) {
        let config=WKSnapshotConfiguration();config.rect=web.bounds
        web.takeSnapshot(with:config) {image,error in
            guard let image=image,let tiff=image.tiffRepresentation,let bitmap=NSBitmapImageRep(data:tiff),let png=bitmap.representation(using:.png,properties:[:]) else {fputs("native snapshot failed: \(String(describing:error))\n",stderr);exit(1)}
            do {try png.write(to:URL(fileURLWithPath:self.output+"/"+name+".png"))} catch {fputs("native write failed\n",stderr);exit(1)}
            print("native WKWebView captured \(name) \(bitmap.pixelsWide)x\(bitmap.pixelsHigh)")
            self.index += 1; self.next()
        }
    }
}
let root=CommandLine.arguments[1],output=CommandLine.arguments[2]
NSApplication.shared.setActivationPolicy(.accessory)
let capture=try Capture(root:root,output:output)
NSApp.run()
