use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("argv") => println!(
            "{}",
            serde_json::to_string(&args.collect::<Vec<_>>()).expect("args")
        ),
        Some("env") => println!(
            "{}",
            serde_json::to_string(&std::env::vars().collect::<std::collections::BTreeMap<_, _>>())
                .expect("env")
        ),
        Some("stdin") => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).expect("stdin");
            println!("{}", bytes.len());
        }
        Some("noise") | Some("controls") => {
            let controls = std::env::args().nth(1).as_deref() == Some("controls");
            let count: usize = args.next().expect("count").parse().expect("count number");
            let byte = if controls { 0 } else { b'x' };
            let writer = std::thread::spawn(move || {
                let mut err = std::io::stderr().lock();
                write_noise(&mut err, count, byte);
            });
            write_noise(&mut std::io::stdout().lock(), count, byte);
            writer.join().expect("writer");
        }
        Some("invalid-utf8") => {
            std::io::stdout()
                .write_all(&[b'a', 255, b'b'])
                .expect("write");
        }
        Some("unicode-boundary") => {
            std::io::stdout()
                .write_all("🦀".repeat(1024).as_bytes())
                .expect("write");
        }
        Some("sleep") => std::thread::sleep(Duration::from_secs(60)),
        Some("ready-file") => {
            std::fs::write(args.next().expect("ready path"), b"ready").expect("ready file");
            std::thread::sleep(Duration::from_secs(60));
        }
        Some("ignore-term") => {
            #[cfg(unix)]
            {
                use std::sync::{
                    Arc,
                    atomic::{AtomicBool, Ordering},
                };
                let terminated = Arc::new(AtomicBool::new(false));
                signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&terminated))
                    .expect("signal handler");
                if let Some(marker) = args.next() {
                    while !terminated.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    std::fs::write(marker, b"term received").expect("term marker");
                }
            }
            println!("ready");
            std::io::stdout().flush().expect("flush");
            std::thread::sleep(Duration::from_secs(60));
        }
        Some("descendant") | Some("descendant-zero") | Some("tree") => {
            let wait = std::env::args().nth(1).as_deref() == Some("tree");
            // This fixture deliberately exits before its descendant. The runner must
            // terminate the group; the test sandbox's PID namespace reaps the orphan.
            #[expect(
                clippy::zombie_processes,
                reason = "exercise leader exit with a live descendant"
            )]
            let mut child = Command::new(std::env::current_exe().expect("fixture path"))
                .arg("sleep")
                .stdin(Stdio::null())
                .spawn()
                .expect("descendant");
            println!("descendant_pid={}", child.id());
            std::io::stdout().flush().expect("flush");
            if wait {
                let _ = child.wait();
            } else {
                let code = if std::env::args().nth(1).as_deref() == Some("descendant-zero") {
                    0
                } else {
                    7
                };
                std::process::exit(code);
            }
        }
        Some("mark") => std::fs::write(args.next().expect("marker"), b"executed").expect("marker"),
        Some("exit") => std::process::exit(args.next().expect("code").parse().expect("integer")),
        _ => panic!("unknown fixture mode"),
    }
}

fn write_noise(writer: &mut impl Write, mut count: usize, byte: u8) {
    let buffer = [byte; 8192];
    while count > 0 {
        let chunk = count.min(buffer.len());
        writer.write_all(&buffer[..chunk]).expect("noise");
        count -= chunk;
    }
}
