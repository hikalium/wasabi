//! A minimal HTTP/1.1 client: one GET request per TCP connection.
extern crate alloc;

use crate::result::Result;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

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
