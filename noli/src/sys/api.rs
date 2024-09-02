pub use sabi::MouseEvent;
pub use sabi::RawIpV4Addr;

/// impl can be found at:
/// - src/sys/wasabi.rs
/// - src/sys/linux.rs
pub trait SystemApi {
    fn exit(_code: u64) -> ! {
        unimplemented!()
    }
    fn write_string(_s: &str) -> u64 {
        unimplemented!()
    }
    fn draw_point(_x: i64, _y: i64, _c: u32) -> u64 {
        unimplemented!();
    }
    fn noop() -> u64 {
        unimplemented!()
    }
    /// Returns None if no key was in the queue.
    /// This may yield the execution to the OS.
    fn read_key() -> Option<char> {
        unimplemented!()
    }
    /// Returns Some if there is a new event, or None.
    /// This may yield the execution to the OS.
    fn get_mouse_cursor_info() -> Option<MouseEvent> {
        unimplemented!()
    }
    /// Returns Some if there is an args region.
    fn get_args_region() -> Option<&'static [u8]> {
        unimplemented!()
    }
    /// Returns 0 if there is a response. Non-zero otherwise.
    /// -2: NXDOMAIN
    /// -1: RESOLUTION_FAILED
    fn nslookup(_host: &str, _result: &mut [RawIpV4Addr]) -> i64 {
        #[cfg(test)]
        {
            if _host == "nolitest.example.com" {
                _result[0] = [127, 0, 0, 1];
                return 1;
            } else if _host == "example.invalid" {
                // c.f. https://www.rfc-editor.org/rfc/rfc6761.html
                // >  The domain "invalid." and any names falling within ".invalid." are special in the ways listed below.
                // > Users MAY assume that queries for "invalid" names will always return NXDOMAIN responses.
                // > Name resolution APIs and libraries SHOULD recognize "invalid" names as special and SHOULD always return immediate negative responses.
                return -2;
            }
        }
        unimplemented!()
    }
    /// Returns a non-negative handle for the socket.
    /// -1: OPEN_FAILED
    fn open_tcp_socket(_ip: RawIpV4Addr, _port: u16) -> i64 {
        unimplemented!()
    }
    /// Returns a non-negative byte size that is queued to be sent.
    /// -1: NO_SUCH_SOCKET
    /// -2: WRITE_ERROR
    fn write_to_tcp_socket(_handle: i64, _buf: &[u8]) -> i64 {
        unimplemented!()
    }
    /// Returns a non-negative byte size that is written to the given buffer.
    /// -1: NO_SUCH_SOCKET
    /// -2: READ_ERROR
    fn read_from_tcp_socket(_handle: i64, _buf: &mut [u8]) -> i64 {
        unimplemented!()
    }
}
