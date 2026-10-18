use std::{env, fs, io::Write, process};

#[link(name = "crypt32")]
extern "system" {
    fn CryptStringToBinaryW(
        text: *const u16, chars: u32, flags: u32, output: *mut u8, bytes: *mut u32,
        skip: *mut u32, flags_out: *mut u32,
    ) -> i32;
}

fn reject() -> ! { process::exit(64) }

fn utf16le(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 6 || args[0] != "-NoProfile" || args[1] != "-NonInteractive"
        || args[2] != "-WindowStyle" || args[3] != "Hidden" || args[4] != "-EncodedCommand" || args[5].len() > 32768 { reject(); }
    let encoded = args[5].to_string_lossy();
    let wide: Vec<u16> = encoded.encode_utf16().collect();
    let mut size = 0u32;
    if unsafe { CryptStringToBinaryW(wide.as_ptr(), wide.len() as u32, 1, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), std::ptr::null_mut()) } == 0
        || size > 32768 { reject(); }
    let mut decoded = vec![0u8; size as usize];
    if unsafe { CryptStringToBinaryW(wide.as_ptr(), wide.len() as u32, 1, decoded.as_mut_ptr(), &mut size, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 { reject(); }
    decoded.truncate(size as usize);
    let expected = concat!(
        "function Get-TaskQInstances($service,$task) { $service.GetRunningTasks(1) | Where-Object { $_.Path -eq $task.Path } }\n",
        "$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=New-Object -ComObject 'Schedule.Service';$s.Connect();foreach($t in $s.GetFolder('\\').GetTasks(1)){$t.Name}"
    );
    if decoded != utf16le(expected) {
        // Only inventory is simulated; ownership and other native reads retain their real behavior.
        let native = std::path::PathBuf::from(env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let status = process::Command::new(native).args(&args).status().unwrap();
        process::exit(status.code().unwrap_or(1));
    }
    let calls = env::current_exe().unwrap().with_extension("calls");
    fs::OpenOptions::new().create(true).append(true).open(calls).unwrap().write_all(b"read\n").unwrap();
}

