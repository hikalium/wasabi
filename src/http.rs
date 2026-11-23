//! A minimal HTTP/1.1 client: one GET request per TCP connection.
extern crate alloc;

use crate::executor::sleep;
use crate::hpet::global_timestamp;
use crate::ip::IpV4Addr;
use crate::net;
use crate::nic;
use crate::result::Result;
use crate::tcp::TcpSocketState;
use crate::tcp::TCP_CLIENT_SOCKET;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

/// What the server sent back, split at the empty line that ends the
/// header.
pub struct HttpResponse {
    /// The status code of the first line: 200 for "HTTP/1.1 200 OK".
    pub status: u16,
    /// The status line and the header fields, without the empty line.
    pub header: String,
    pub body: Vec<u8>,
}

/// Build the text of a GET request. HTTP/1.1 requires the Host field.
/// "Connection: close" asks the server to close the connection after
/// the response, which tells the client that the response is complete.
pub fn build_get_request(host: &str, path: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Connection: close\r\n\
         \r\n"
    )
}

pub fn parse_response(raw: &[u8]) -> Result<HttpResponse> {
    let header_len = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("HTTP: the response has no end of header")?;
    let header = core::str::from_utf8(&raw[..header_len])
        .map_err(|_| "HTTP: the header is not valid UTF-8")?;
    let status = header
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("HTTP: the status line has no status code")?;
    Ok(HttpResponse {
        status,
        header: String::from(header),
        body: raw[header_len + 4..].to_vec(),
    })
}

/// Send "GET `path`" to the HTTP server at `addr` and return its
/// response. `host` goes into the Host field; one server address can
/// serve many host names and uses it to tell them apart.
pub async fn http_get(
    addr: IpV4Addr,
    host: &str,
    path: &str,
) -> Result<HttpResponse> {
    const HTTP_PORT: u16 = 80;
    const TIMEOUT: Duration = Duration::from_secs(10);
    let sock = &TCP_CLIENT_SOCKET;
    let our_mac = nic::our_mac().ok_or("NCM not ready (no MAC)")?;
    if !nic::has_ip() {
        return Err("no IP yet (waiting for DHCP)");
    }
    let server_mac = net::next_hop_mac(our_mac, addr).await?;
    let deadline = global_timestamp() + TIMEOUT;
    let syn = sock.connect(
        our_mac,
        nic::our_ip(),
        server_mac,
        addr,
        HTTP_PORT,
        global_timestamp(),
    )?;
    nic::enqueue_tx_frame(syn);
    while sock.state() == TcpSocketState::SynSent {
        if global_timestamp() >= deadline {
            sock.abort();
            return Err("HTTP: timeout while connecting");
        }
        sleep(Duration::from_millis(10)).await;
    }
    if sock.state() != TcpSocketState::Established {
        return Err("HTTP: the server refused the connection");
    }
    sock.push_tx_bytes(build_get_request(host, path).as_bytes());

    // Read until the server closes the connection. Look at the state
    // before draining: once the socket is Closed nothing more arrives,
    // so the bytes drained after that are the whole rest of the response.
    let mut raw = Vec::new();
    loop {
        let closed = sock.state() == TcpSocketState::Closed;
        raw.extend(core::iter::from_fn(|| sock.pop_rx_byte()));
        if closed {
            return parse_response(&raw);
        }
        if global_timestamp() >= deadline {
            sock.abort();
            return Err("HTTP: timeout while waiting for the response");
        }
        sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn build_get_request_ends_with_an_empty_line() {
        assert_eq!(
            build_get_request("example.com", "/index.html"),
            "GET /index.html HTTP/1.1\r\n\
             Host: example.com\r\n\
             Connection: close\r\n\
             \r\n"
        );
    }

    #[test_case]
    fn parse_response_splits_header_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let res = parse_response(raw).unwrap();
        assert_eq!(res.status, 200);
        assert_eq!(res.header, "HTTP/1.1 200 OK\r\nContent-Length: 5");
        assert_eq!(res.body, b"hello");
        // The body may be empty, and may itself contain empty lines.
        let res = parse_response(b"HTTP/1.1 404 Not Found\r\n\r\n").unwrap();
        assert_eq!((res.status, res.body.len()), (404, 0));
        let res = parse_response(b"HTTP/1.1 200 OK\r\n\r\na\r\n\r\nb").unwrap();
        assert_eq!(res.body, b"a\r\n\r\nb");
    }

    #[test_case]
    fn parse_response_rejects_what_is_not_a_response() {
        // Cut off before the end of the header.
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Le").is_err());
        assert!(parse_response(b"").is_err());
        // No status code where one belongs.
        assert!(parse_response(b"hello\r\n\r\n").is_err());
        assert!(parse_response(b"HTTP/1.1 OK\r\n\r\n").is_err());
    }
}
