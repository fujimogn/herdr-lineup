//! Request-scoped newline-framed connections pinned to one Unix socket endpoint.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointIdentity {
    pub device: u64,
    pub inode: u64,
}
impl EndpointIdentity {
    pub fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other(
                "endpoint must be a socket owned by this user",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}
pub struct Transport {
    path: PathBuf,
    identity: EndpointIdentity,
    reader: BufReader<UnixStream>,
    response_complete: bool,
}
impl Transport {
    pub fn connect(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other("HERDR_SOCKET_PATH must be absolute"));
        }
        let identity = EndpointIdentity::read(path)?;
        let stream = connect_stream(path)?;
        let transport = Self {
            path: path.into(),
            identity,
            reader: BufReader::new(stream),
            response_complete: false,
        };
        transport.verify_endpoint()?;
        Ok(transport)
    }
    pub fn reconnect_same_endpoint(&mut self) -> io::Result<()> {
        self.verify_endpoint()?;
        let stream = connect_stream(&self.path)?;
        self.verify_endpoint()?;
        self.reader = BufReader::new(stream);
        self.response_complete = false;
        Ok(())
    }
    pub fn identity(&self) -> EndpointIdentity {
        self.identity
    }
    pub fn verify_endpoint(&self) -> io::Result<()> {
        if EndpointIdentity::read(&self.path)? != self.identity {
            return Err(io::Error::other(
                "Herdr socket endpoint was replaced; refusing to reconnect",
            ));
        }
        Ok(())
    }
    pub fn send_line(&mut self, frame: &[u8]) -> io::Result<()> {
        self.verify_endpoint()?;
        if frame.len() > MAX_FRAME_BYTES || frame.contains(&b'\n') {
            return Err(io::Error::other("invalid outbound frame"));
        }
        // Herdr's ordinary JSON API closes after one response. Never replay
        // a possibly accepted request: open the next connection before sending.
        if self.response_complete {
            self.reconnect_same_endpoint()?;
        }
        write_frame(self.reader.get_mut(), frame)?;
        self.verify_endpoint()
    }
    pub fn receive_line(&mut self) -> io::Result<Vec<u8>> {
        self.verify_endpoint()?;
        let frame = read_frame(&mut self.reader, MAX_FRAME_BYTES)?;
        self.verify_endpoint()?;
        self.response_complete = true;
        Ok(frame)
    }
    /// Detect unexpected disconnect; EOF after a full response is normal.
    pub fn check_connection(&self) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        self.verify_endpoint()?;
        if self.response_complete {
            return Ok(());
        }
        let mut byte = 0u8;
        // MSG_DONTWAIT avoids changing the shared stream's blocking mode.
        let result = unsafe {
            libc::recv(
                self.reader.get_ref().as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        if result == 0 {
            return Err(io::Error::other("Herdr socket disconnected"));
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
/// Connect with a real deadline, including a saturated Unix listen backlog.
pub(crate) fn connect_stream(path: &Path) -> io::Result<UnixStream> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: zero is valid for sockaddr_un; sun_path remains NUL terminated.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::other(
            "Unix socket path is too long or contains NUL",
        ));
    }
    address.sun_family = libc::AF_UNIX as _;
    for (destination, source) in address.sun_path.iter_mut().zip(bytes) {
        *destination = *source as _;
    }
    #[cfg(target_os = "macos")]
    {
        address.sun_len = std::mem::size_of::<libc::sockaddr_un>() as u8;
    }
    // SAFETY: a successful socket fd is immediately owned by UnixStream.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    unsafe {
        if libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    stream.set_nonblocking(true)?;
    let status = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of::<libc::sockaddr_un>() as _,
        )
    };
    if status < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "socket connect deadline exceeded",
                ));
            }
            let mut pollfd = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().max(1) as i32) };
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            if result == 0 {
                continue;
            }
            let mut error = 0i32;
            let mut size = std::mem::size_of::<i32>() as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut error as *mut i32).cast(),
                    &mut size,
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
            }
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            break;
        }
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}

/// Wait against one monotonic deadline without changing socket options.
fn wait_ready(stream: &UnixStream, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "socket frame deadline exceeded",
            ));
        }
        let mut descriptor = libc::pollfd {
            fd: stream.as_raw_fd(),
            events,
            revents: 0,
        };
        // SAFETY: descriptor is valid and only borrowed for this bounded poll.
        let result = unsafe { libc::poll(&mut descriptor, 1, remaining.as_millis().max(1) as i32) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if result > 0 {
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::other("invalid socket descriptor"));
            }
            // Let read/write observe buffered bytes, EOF, or the actual error.
            return Ok(());
        }
    }
}

pub(crate) fn write_frame(stream: &mut UnixStream, frame: &[u8]) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(frame.len() + 1);
    bytes.extend_from_slice(frame);
    bytes.push(b'\n');
    let deadline = Instant::now() + Duration::from_millis(500);
    // fcntl affects the local fd, unlike setsockopt which macOS rejects after
    // peer shutdown (EINVAL). Nonblocking I/O plus poll preserves the deadline.
    stream.set_nonblocking(true)?;
    let mut remaining_bytes = bytes.as_slice();
    while !remaining_bytes.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "socket write deadline exceeded",
            ));
        }
        wait_ready(stream, libc::POLLOUT, deadline)?;
        match stream.write(remaining_bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "socket write failed",
                ));
            }
            Ok(count) => remaining_bytes = &remaining_bytes[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn read_frame(reader: &mut BufReader<UnixStream>, limit: usize) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_millis(500);
    reader.get_mut().set_nonblocking(true)?;
    let mut frame = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "socket frame deadline exceeded",
            ));
        }
        // BufReader may already contain the next frame: do not wait for more
        // kernel data before consuming it. Peer EOF still permits buffered data.
        if reader.buffer().is_empty() {
            wait_ready(reader.get_ref(), libc::POLLIN, deadline)?;
        }
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if buffer.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "socket frame is incomplete",
            ));
        }
        let end = buffer.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(buffer.len(), |end| end + 1);
        if frame.len() + count > limit {
            return Err(io::Error::other("socket frame is too large"));
        }
        frame.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            frame.pop();
            return Ok(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnects_only_to_unchanged_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let (peer, _) = listener.accept().unwrap();
        drop(peer);
        assert!(transport.check_connection().is_err());
        transport.reconnect_same_endpoint().unwrap();
        let _new_peer = listener.accept().unwrap();
        assert!(transport.check_connection().is_ok());
        std::fs::remove_file(&path).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        replacement.set_nonblocking(true).unwrap();
        assert!(transport.reconnect_same_endpoint().is_err());
        assert!(replacement.accept().is_err());
    }
    #[test]
    fn completed_response_allows_eof_but_never_a_replacement_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        transport.send_line(b"first-request").unwrap();
        peer.write_all(b"complete-response\n").unwrap();
        drop(peer);
        assert_eq!(transport.receive_line().unwrap(), b"complete-response");
        assert!(transport.check_connection().is_ok());
        std::fs::remove_file(&path).unwrap();
        // Keep the original listener alive so its inode cannot be reused.
        let replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        replacement.set_nonblocking(true).unwrap();
        assert!(transport.send_line(b"next-request").is_err());
        assert!(replacement.accept().is_err());
    }

    #[test]
    fn frame_deadlines_do_not_mutate_socket_timeout_options() {
        let (mut client, mut peer) = UnixStream::pair().unwrap();
        let original = Some(Duration::from_secs(3));
        client.set_read_timeout(original).unwrap();
        client.set_write_timeout(original).unwrap();
        write_frame(&mut client, b"request").unwrap();
        assert_eq!(client.write_timeout().unwrap(), original);
        let mut request = String::new();
        BufReader::new(peer.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        peer.write_all(b"response\n").unwrap();
        drop(peer);
        let mut reader = BufReader::new(client);
        assert_eq!(read_frame(&mut reader, 8192).unwrap(), b"response");
        assert_eq!(reader.get_ref().read_timeout().unwrap(), original);
    }

    #[test]
    fn frame_reader_consumes_buffered_frames_after_peer_eof() {
        let (client, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"first\nsecond\n").unwrap();
        drop(peer);
        let mut reader = BufReader::new(client);
        assert_eq!(read_frame(&mut reader, 8192).unwrap(), b"first");
        assert_eq!(read_frame(&mut reader, 8192).unwrap(), b"second");
        assert!(read_frame(&mut reader, 8192).is_err());
    }

    #[test]
    fn stalled_peer_cannot_extend_write_deadline() {
        let (mut client, _peer) = UnixStream::pair().unwrap();
        let started = Instant::now();
        let error = write_frame(&mut client, &vec![b'x'; MAX_FRAME_BYTES]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn slow_trickle_cannot_extend_frame_deadline() {
        let (read, mut write) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            for _ in 0..4 {
                let _ = write.write_all(b"x");
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        let started = Instant::now();
        let mut reader = BufReader::new(read);
        assert!(read_frame(&mut reader, 8192).is_err());
        assert!(started.elapsed() < Duration::from_millis(750));
        drop(reader);
        writer.join().unwrap();
    }
    #[test]
    fn replacement_is_rejected_before_further_io() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let _peer = listener.accept().unwrap();
        std::fs::remove_file(&path).unwrap();
        let _replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(transport.send_line(b"{}").is_err());
        assert!(transport.verify_endpoint().is_err());
    }
}
