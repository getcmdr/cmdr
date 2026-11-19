//! A PAC file served on loopback, for the PAC cells on every OS that runs PAC files.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use reqwest::Url;

/// A PAC script that sends `example.com` through `proxy.test:3128` (then direct) and every other
/// host direct.
pub(crate) const SCRIPT: &str = r#"function FindProxyForURL(url, host) {
  if (host == "example.com") return "PROXY proxy.test:3128; DIRECT";
  return "DIRECT";
}"#;

/// Serves `body` as a PAC file on `127.0.0.1`. A `None` body accepts connections and never
/// answers, like a PAC host that hangs.
pub(crate) fn pac_server(body: Option<&'static str>) -> String {
    pac_server_after(body, Duration::ZERO)
}

/// [`pac_server`], answering each connection only after `delay`, like a PAC host on a loaded
/// network.
pub(crate) fn pac_server_after(body: Option<&'static str>, delay: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("a bound address");
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Some(body) = body else {
                held.push(stream);
                continue;
            };
            std::thread::spawn(move || {
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                // allowed-test-sleep: the fake latency of a slow PAC host is the subject
                std::thread::sleep(delay);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            });
        }
    });
    format!("http://{address}/proxy.pac")
}

pub(crate) fn url(s: &str) -> Url {
    Url::parse(s).expect("a valid test URL")
}
