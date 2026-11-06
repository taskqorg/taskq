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

    // Chain: M6. UX30-20/22: one linear event scale, no minimum heights, and no invented arrivals in legacy history.
    #[test]
    fn selected_event_stack_uses_quantities_on_every_surface() {
        let mut data = Data {counts:[Some(1);4], blocked:Some(0), paused:false, bins:vec![
            Bin {incoming:0,incoming_missing:vec![],start:0.0,end:1.0,counts:[2,1,3,99],missing:vec![],ongoing:true},
        ]};
        assert_eq!(peak(&data.bins), 6, "Completed + Ask + Failed, never Blocked");
        data.bins[0].incoming=1;
        assert_eq!(peak(&data.bins),7);
        for dark in [false,true] {
            let vector = svg(&data, dark);
            let tree=resvg::usvg::Tree::from_str(&vector,&resvg::usvg::Options::default()).unwrap();
            assert_eq!(tree.size().height(),22.0);
            let mut bottom=20.5;
            for (mark,n,color) in [("completed",2,palette(dark).work),("new",1,palette(dark).incoming),("ask",3,palette(dark).ask),("failed",1,palette(dark).failed)] {
                let h=9.0*n as f64/7.0;bottom-=h;
                assert!(vector.contains(&format!(r#"data-mark="{mark}" x="1" y="{bottom}" width="2.5" height="{h}" fill="{}""#,hex(color))),"{vector}");
            }
            for mark in ["zero","ongoing"] {assert!(!vector.contains(&format!(r#"data-mark="{mark}""#)));}
            assert!(!vector.contains(r#"visibility="hidden""#), "Ask is shared with the small icon");
        }
        data.bins[0].counts=[u32::MAX,1,0,0];data.bins[0].incoming=u32::MAX;
        assert_eq!(peak(&data.bins),2*u32::MAX as u64+1,"sum cannot overflow");
        let vector=svg(&data,false);
        assert!(vector.contains(&format!(r#"height="{}""#,9.0/(2*u32::MAX as u64+1) as f64)),"rare failed event keeps its actual subpixel height");
        data.bins[0].incoming_missing=vec!["legacy New unavailable".into()];
        assert_eq!(peak(&data.bins),u32::MAX as u64+1,"legacy known series retain a usable scale");
        let vector=svg(&data,false);
        assert!(vector.contains(r#"data-mark="completed""#)&&vector.contains(r#"data-mark="failed""#)&&vector.contains(r#"data-mark="unknown""#));
        assert!(!vector.contains(r#"data-mark="new""#));
    }

    // Chain: I1/I2/I6. Presentation order must not swap source values, sums or palette meanings.
    #[test]
    fn summary_counter_order_preserves_values_and_colors() {
        for dark in [false,true] {
            let p=palette(dark);
            for (case,(counts,blocked,labels)) in [
                ([Some(1),Some(2),Some(1),Some(0)],Some(2),["3","2","1"]),
                ([Some(0);4],Some(0),["","",""]),
                ([None;4],None,["–","–","–"]),
                ([Some(100),Some(99),Some(98),Some(0)],Some(2),["99+","99","99+"]),
                ([Some(10),Some(0),None,Some(0)],Some(3),["–","","10"]),
            ].into_iter().enumerate() {
                let vector=svg(&Data {counts,blocked,bins:vec![],paused:false},dark);
                if let Ok(dir)=std::env::var("TASKQ_ICON_EVIDENCE") {
                    let dir=std::path::Path::new(&dir);std::fs::create_dir_all(dir).unwrap();
                    let name=format!("counter-{case}-{}",if dark {"dark"}else{"light"});
                    std::fs::write(dir.join(format!("{name}.svg")),&vector).unwrap();
                    for size in [44,256] {std::fs::write(dir.join(format!("{name}-{size}.png")),raster_slot(&vector,size).unwrap().png()).unwrap();}
                }
                let tree=resvg::usvg::Tree::from_str(&vector,&resvg::usvg::Options::default()).unwrap();
                for (i,(label,color)) in labels.into_iter().zip([p.muted,p.ask,p.work]).enumerate() {
                    let id=format!("counter-{i}");
                    if label.is_empty() {assert!(tree.node_by_id(&id).is_none());continue;}
                    let group=vector.split(&format!("id=\"{id}\"")).nth(1).unwrap();
                    let paths=counter(label,hex(color)).0;
                    assert!(group.split_once('>').unwrap().1.starts_with(&paths),"cell {i} must show {label} with its semantic color");
                }
            }
        }
    }

    // #1789 A1/A2: exact labels and measured ink must survive every position without collisions or clipping.
    #[test]
    fn exact_two_digit_counters_reclaim_space_before_widening() {
        let bins=(0..6).map(|i|Bin {incoming:0,incoming_missing:vec![],start:i as f64,end:(i+1) as f64,counts:[1,0,0,0],missing:vec![],ongoing:i==5}).collect();
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
                        if data.counts[[2,1,0][i]]==Some(0) {assert!(node.is_none());continue;}
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

    // Chain: M6. Shared event geometry survives every viewport; incomplete bins and Blocked never set the scale.
    #[test]
    fn six_columns_keep_quantity_marks_palette_and_device_bounds() {
        let bins:Vec<Bin>=[4,0,999,12,7,4].into_iter().enumerate().map(|(i,c)|Bin {incoming:0,incoming_missing:vec![],start:i as f64,end:(i+1) as f64,counts:[c,0,0,0],missing:if i==2 {vec!["synthetic missing source".into()]}else{vec![]},ongoing:i==5}).collect();
        let mut data=Data {counts:[Some(0);4],blocked:Some(0),bins,paused:false};
        assert!(raster("",44).is_err(),"missing published SVG stays a render error");
        for dark in [false,true] {
            let svg=svg(&data,dark);
            assert_eq!(svg.matches("data-start=").count(),6);
            assert!(svg.contains(r#"y="17.5" width="2.5" height="3""#),"4 / peak 12");
            assert_eq!(svg.matches(r#"data-mark="unknown""#).count(),1);
            assert_eq!(svg.matches(r#"data-mark="zero""#).count(),0);
            assert_eq!(svg.matches(r#"data-mark="ongoing""#).count(),0);
            assert!(svg.contains(&hex(palette(dark).work)));
            for n in [16,18,22,33,44,66,256] {let image=raster(&svg,n).unwrap();assert_eq!((image.w,image.h),(n,n));assert!(image.px.chunks_exact(4).any(|p|p[3]>0));}
        }
        assert_eq!(peak(&data.bins),12,"incomplete 999 cannot set the scale");
        for (dark,expected) in [(false,[[8,127,91,255],[153,82,0,255],[201,38,65,255],[83,97,113,255],[130,80,223,255]]),(true,[[56,232,160,255],[255,194,71,255],[255,99,120,255],[185,198,216,255],[188,163,235,255]])] {
            let p=palette(dark);assert_eq!([p.work,p.ask,p.failed,p.muted,p.review],expected,"approved palette A");
        }
        assert!(css(false).ends_with(";--review:#8250DF")&&css(true).ends_with(";--review:#BCA3EB"),"Review accent token");
        let before=svg(&data,false);data.bins[0].counts[3]=6;
        assert_eq!(svg(&data,false),before,"Blocked is exact detail only");
        data.bins[0].counts=[4,8,7,6];
        let after=svg(&data,false);
        assert!(after.contains(r#"data-mark="failed""#));
        assert_eq!(peak(&data.bins),19,"Completed + Failed + Ask share the scale");
        for b in &mut data.bins {b.counts=[0;4];b.missing.clear();}
        let zero=svg(&data,false);assert_eq!(zero.matches(r#"data-mark="zero""#).count(),0);assert!(!zero.contains("NaN"));
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
