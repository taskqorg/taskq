    use super::*;
    use std::os::windows::ffi::OsStrExt;
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(name:*const u16,open:u32,mode:u32,max:u32,out:u32,input:u32,timeout:u32,security:Handle)->Handle;
        fn ConnectNamedPipe(pipe:Handle,overlapped:Handle)->i32;
    }
    pub struct Listener(File);
    impl Listener {
        pub fn bind(path:&Path)->io::Result<Self> {
            let wide:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();
            let raw=unsafe {CreateNamedPipeW(wide.as_ptr(),3|0x00080000,1|8,1,4096,4096,0,std::ptr::null_mut())};
            if raw as isize == -1 {return Err(io::Error::last_os_error());}
            Ok(Self(unsafe {File::from_raw_handle(raw)}))
        }
        pub fn accept(self)->io::Result<(Stream,())> {
            let until=Instant::now()+Duration::from_secs(3);
            loop {
                let connected=unsafe {ConnectNamedPipe(self.0.as_raw_handle(),std::ptr::null_mut())};let error=io::Error::last_os_error();
                if connected!=0 || error.raw_os_error()==Some(535) {
                    let stream=Stream::from_file(self.0)?;
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                    return Ok((stream,()));
                }
                if !matches!(error.raw_os_error(),Some(536|232)) {return Err(error);}
                pause(until)?;
            }
        }
    }
    // Chain: silent native peer -> saturated client writes -> bounded timeout, no retained I/O.
    #[test]
    fn native_pipe_bounds_unread_writes_and_empty_reads() {
        let remote=Stream::connect(Path::new(r"\\other-host\pipe\codex-ipc"),Instant::now()+Duration::from_secs(1)).err().unwrap();
        assert!(remote.to_string().contains("must be local"),"remote endpoints must be rejected before connection");
        for mode in ["read","write"] {
            let path=std::path::PathBuf::from(format!(r"\\.\pipe\taskq-pipe-{}",super::super::super::uuid()));
            let listener=Listener::bind(&path).unwrap();
            let (finished,completion)=std::sync::mpsc::channel();
            let (ready,started)=std::sync::mpsc::channel();
            let peer=std::thread::spawn(move|| {let (_stream,_)=listener.accept().unwrap();ready.send(()).unwrap();let _=completion.recv_timeout(Duration::from_secs(5));});
            let mut stream=Stream::connect(&path,Instant::now()+Duration::from_secs(1)).unwrap();
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();stream.set_write_timeout(Some(Duration::from_millis(100))).unwrap();
            let began=Instant::now();
            let error=if mode=="read" {stream.read_exact(&mut [0]).unwrap_err()} else {stream.write_all(&vec![0;1024*1024]).unwrap_err()};
            assert_eq!(error.kind(),io::ErrorKind::TimedOut,"{mode}: {error}");assert!(began.elapsed()<Duration::from_secs(1));
            drop(stream);let _=finished.send(());peer.join().unwrap();
        }
    }
