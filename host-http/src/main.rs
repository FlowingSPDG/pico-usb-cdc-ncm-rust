use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let peer = stream.peer_addr().ok();

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");

    eprintln!("[host-http] peer={peer:?} request={first_line:?}");

    let body = "Hello from host\n";
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes())?;
    Ok(())
}

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", 8080))?;
    eprintln!("[host-http] listening on 0.0.0.0:8080");

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                if let Err(e) = handle(stream) {
                    eprintln!("[host-http] error: {e}");
                }
            }
            Err(e) => eprintln!("[host-http] accept error: {e}"),
        }
    }

    Ok(())
}

