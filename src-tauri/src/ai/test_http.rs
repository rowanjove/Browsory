//! Bounded loopback-only HTTP fixtures. No user data or external services.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) struct FixtureServer {
    pub base_url: String,
    pub body: Receiver<Vec<u8>>,
    worker: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start(response: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let (sender, body) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            // Windows inherits non-blocking mode from the listener onto accepted streams.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let header_end = loop {
                if let Some(offset) = request.windows(4).position(|p| p == b"\r\n\r\n") {
                    break offset + 4;
                }
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "incomplete fixture headers");
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() < 64 * 1024, "fixture headers exceeded limit");
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            assert!(length <= 8 * 1024 * 1024, "fixture body exceeded limit");
            while request.len() < header_end + length {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "incomplete fixture body");
                request.extend_from_slice(&chunk[..count]);
            }
            sender
                .send(request[header_end..header_end + length].to_vec())
                .unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        });
        Self {
            base_url,
            body,
            worker: Some(worker),
        }
    }

    pub fn json(value: serde_json::Value) -> Self {
        let text = value.to_string();
        Self::start(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()))
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // A request assertion can already be unwinding. Never cause a
            // second panic in Drop; missing fixture data fails recv_timeout.
            let _ = worker.join();
        }
    }
}
