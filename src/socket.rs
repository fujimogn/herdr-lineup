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
    reusable: bool,
}
impl Transport {
    pub fn connect(path: &Path) -> io::Result<Self> {
        Self::connect_with_deadline(path, None)
    }
    /// Share the caller's absolute deadline; connecting still has a one-second cap.
    pub fn connect_before(path: &Path, deadline: Instant) -> io::Result<Self> {
        Self::connect_with_deadline(path, Some(deadline))
    }
    fn connect_with_deadline(path: &Path, deadline: Option<Instant>) -> io::Result<Self> {
        check_deadline(deadline)?;
        if !path.is_absolute() {
            return Err(io::Error::other("HERDR_SOCKET_PATH must be absolute"));
        }
        let identity = EndpointIdentity::read(path)?;
        let stream = match deadline {
            Some(deadline) => connect_stream_before(path, deadline)?,
            None => connect_stream(path)?,
        };
        let transport = Self {
            path: path.into(),
            identity,
            reader: BufReader::new(stream),
            response_complete: false,
            reusable: true,
        };
        transport.verify_endpoint()?;
        check_deadline(deadline)?;
        Ok(transport)
    }
    pub fn reconnect_same_endpoint(&mut self) -> io::Result<()> {
        self.reconnect_with_deadline(None)
    }
    /// Explicit recovery discards the previous stream, never replaying its request.
    pub fn reconnect_same_endpoint_before(&mut self, deadline: Instant) -> io::Result<()> {
        self.reconnect_with_deadline(Some(deadline))
    }
    fn reconnect_with_deadline(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        check_deadline(deadline)?;
        // Failed recovery must not make the old, possibly ambiguous stream usable.
        self.reusable = false;
        self.verify_endpoint()?;
        let stream = match deadline {
            Some(deadline) => connect_stream_before(&self.path, deadline)?,
            None => connect_stream(&self.path)?,
        };
        self.verify_endpoint()?;
        check_deadline(deadline)?;
        self.reader = BufReader::new(stream);
        self.response_complete = false;
        self.reusable = true;
        Ok(())
    }
    fn require_reusable(&self) -> io::Result<()> {
        if !self.reusable {
            return Err(io::Error::other(
                "socket requires explicit reconnect after failure",
            ));
        }
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
    /// Missing paths are transient; only a different observed inode proves replacement.
    pub fn endpoint_replaced(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.dev() != self.identity.device || metadata.ino() != self.identity.inode
        })
    }
    pub fn send_line(&mut self, frame: &[u8]) -> io::Result<()> {
        self.send_with_deadline(frame, None)
    }
    /// Reconnect (after a complete response) and write share the caller's budget.
    pub fn send_line_before(&mut self, frame: &[u8], deadline: Instant) -> io::Result<()> {
        self.send_with_deadline(frame, Some(deadline))
    }
    fn send_with_deadline(&mut self, frame: &[u8], deadline: Option<Instant>) -> io::Result<()> {
        check_deadline(deadline)?;
        self.require_reusable()?;
        if frame.len() > MAX_FRAME_BYTES || frame.contains(&b'\n') {
            return Err(io::Error::other("invalid outbound frame"));
        }
        let result = (|| {
            self.verify_endpoint()?;
            // Herdr closes after one response. Only a completed response permits
            // automatic reconnect; an uncertain request is never replayed.
            if self.response_complete {
                self.reconnect_with_deadline(deadline)?;
            }
            match deadline {
                Some(deadline) => write_frame_before(self.reader.get_mut(), frame, deadline)?,
                None => write_frame(self.reader.get_mut(), frame)?,
            }
            self.response_complete = false;
            self.verify_endpoint()?;
            check_deadline(deadline)
        })();
        if result.is_err() {
            self.reusable = false;
        }
        result
    }
    pub fn receive_line(&mut self) -> io::Result<Vec<u8>> {
        self.receive_with_deadline(None)
    }
    /// Use the same absolute deadline passed to connect/send for one request.
    pub fn receive_line_before(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        self.receive_with_deadline(Some(deadline))
    }
    fn receive_with_deadline(&mut self, deadline: Option<Instant>) -> io::Result<Vec<u8>> {
        self.require_reusable()?;
        let result = (|| {
            // Even a preflight read timeout abandons a possibly accepted request.
            check_deadline(deadline)?;
            self.verify_endpoint()?;
            let frame = match deadline {
                Some(deadline) => read_frame_before(&mut self.reader, MAX_FRAME_BYTES, deadline)?,
                None => read_frame(&mut self.reader, MAX_FRAME_BYTES)?,
            };
            self.verify_endpoint()?;
            check_deadline(deadline)?;
            self.response_complete = true;
            Ok(frame)
        })();
        if result.is_err() {
            self.reusable = false;
        }
        result
    }
    /// Detect unexpected disconnect; EOF after a full response is normal.
    pub fn check_connection(&self) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        self.require_reusable()?;
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
    connect_stream_before(path, Instant::now() + Duration::from_secs(1))
}

pub(crate) fn connect_stream_before(
    path: &Path,
    caller_deadline: Instant,
) -> io::Result<UnixStream> {
    let deadline = capped_deadline(caller_deadline, Duration::from_secs(1))?;
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
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "socket connect deadline exceeded",
            ));
        }
        // SAFETY: a successful socket fd is immediately owned by UnixStream.
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let stream = unsafe { UnixStream::from_raw_fd(fd) };
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        stream.set_nonblocking(true)?;
        check_deadline(Some(deadline))?;
        let status = unsafe {
            libc::connect(
                fd,
                (&address as *const libc::sockaddr_un).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as _,
            )
        };
        if status < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EAGAIN) {
                // Linux reports EAGAIN when the Unix listen backlog is full.
                // poll may report POLLOUT|POLLHUP and SO_ERROR=0 for that fd
                // even though it never connected; retry with a fresh fd.
                drop(stream);
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
                continue;
            }
            if error.raw_os_error() != Some(libc::EINPROGRESS) {
                return Err(error);
            }
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
                let result =
                    unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis().max(1) as i32) };
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
        check_deadline(Some(deadline))?;
        stream.set_nonblocking(false)?;
        return Ok(stream);
    }
}

fn check_deadline(deadline: Option<Instant>) -> io::Result<()> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "socket deadline exceeded",
        ));
    }
    Ok(())
}

fn capped_deadline(caller: Instant, local_cap: Duration) -> io::Result<Instant> {
    check_deadline(Some(caller))?;
    Ok(caller.min(Instant::now() + local_cap))
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
    write_frame_before(stream, frame, Instant::now() + Duration::from_millis(500))
}

pub(crate) fn write_frame_before(
    stream: &mut UnixStream,
    frame: &[u8],
    caller_deadline: Instant,
) -> io::Result<()> {
    let deadline = capped_deadline(caller_deadline, Duration::from_millis(500))?;
    let mut bytes = Vec::with_capacity(frame.len() + 1);
    bytes.extend_from_slice(frame);
    bytes.push(b'\n');
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
        check_deadline(Some(deadline))?;
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
    check_deadline(Some(deadline))
}

pub(crate) fn read_frame(reader: &mut BufReader<UnixStream>, limit: usize) -> io::Result<Vec<u8>> {
    read_frame_before(reader, limit, Instant::now() + Duration::from_millis(500))
}

pub(crate) fn read_frame_before(
    reader: &mut BufReader<UnixStream>,
    limit: usize,
    caller_deadline: Instant,
) -> io::Result<Vec<u8>> {
    let deadline = capped_deadline(caller_deadline, Duration::from_millis(500))?;
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
        check_deadline(Some(deadline))?;
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
            check_deadline(Some(deadline))?;
            return Ok(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_api_preserves_descriptor_and_frame_boundaries() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let client = connect_stream_before(&path, deadline).unwrap();
        assert_ne!(
            unsafe { libc::fcntl(client.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        let (mut peer, _) = listener.accept().unwrap();
        peer.write_all(b"1234\nsecond\n").unwrap();
        drop(peer);
        let mut reader = BufReader::new(client);
        assert_eq!(
            read_frame_before(&mut reader, 5, deadline).unwrap(),
            b"1234"
        );
        assert_eq!(
            read_frame_before(&mut reader, 7, deadline).unwrap(),
            b"second"
        );
        assert_eq!(
            read_frame_before(&mut reader, 7, deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        let (client, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"12345\n").unwrap();
        assert!(read_frame_before(&mut BufReader::new(client), 5, deadline).is_err());
        let mut transport = Transport::connect_before(&path, deadline).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        assert!(transport.send_line_before(b"bad\nframe", deadline).is_err());
        assert!(
            transport
                .send_line_before(&vec![b'x'; MAX_FRAME_BYTES + 1], deadline)
                .is_err()
        );
        // Rejected input never touches the wire or poisons the transport.
        transport.send_line_before(b"valid", deadline).unwrap();
        let mut request = String::new();
        BufReader::new(peer.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        assert_eq!(request, "valid\n");
        peer.write_all(b"ok\n").unwrap();
        assert_eq!(transport.receive_line_before(deadline).unwrap(), b"ok");
    }

    #[test]
    fn caller_deadline_is_not_reset_and_long_budgets_keep_local_caps() {
        let caller = Instant::now() + Duration::from_millis(250);
        assert_eq!(
            capped_deadline(caller, Duration::from_secs(1)).unwrap(),
            caller
        );
        assert_eq!(
            capped_deadline(caller, Duration::from_millis(500)).unwrap(),
            caller
        );
        let before = Instant::now();
        let far = before + Duration::from_secs(60);
        let connect = capped_deadline(far, Duration::from_secs(1)).unwrap();
        let after = Instant::now();
        assert!(
            (before + Duration::from_secs(1)..=after + Duration::from_secs(1)).contains(&connect)
        );
        let before = Instant::now();
        let frame = capped_deadline(far, Duration::from_millis(500)).unwrap();
        let after = Instant::now();
        assert!(
            (before + Duration::from_millis(500)..=after + Duration::from_millis(500))
                .contains(&frame)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reconnect_uses_remaining_caller_budget_with_full_backlog() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let _queued = UnixStream::connect(&path).unwrap();
        let deadline = Instant::now() + Duration::from_millis(150);
        assert_eq!(
            transport
                .reconnect_same_endpoint_before(deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(Instant::now() >= deadline);
        assert!(transport.check_connection().is_err());
        assert!(transport.send_line(b"never").is_err());
        // Draining only the queued connection proves timeout did not connect.
        let (_queued_peer, _) = listener.accept().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let recovery = Instant::now() + Duration::from_secs(5);
        transport.reconnect_same_endpoint_before(recovery).unwrap();
        let (_fresh, _) = listener.accept().unwrap();
        assert!(transport.check_connection().is_ok());
    }

    #[test]
    fn failed_write_requires_explicit_reconnect_without_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        let deadline = Instant::now() + Duration::from_millis(150);
        assert_eq!(
            transport
                .send_line_before(&vec![b'x'; MAX_FRAME_BYTES], deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(transport.send_line(b"no-replay").is_err());
        assert!(transport.receive_line().is_err());
        assert!(transport.check_connection().is_err());
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        transport.reconnect_same_endpoint().unwrap();
        let (fresh, _) = listener.accept().unwrap();
        transport.send_line(b"new-request").unwrap();
        let mut request = String::new();
        BufReader::new(fresh).read_line(&mut request).unwrap();
        assert_eq!(request, "new-request\n");
    }

    #[test]
    fn expired_deadline_starts_no_connect_or_frame() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let expired = Instant::now();
        assert_eq!(
            Transport::connect_before(&path, expired)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            connect_stream_before(&path, expired).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let mut transport = Transport::connect(&path).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        assert_eq!(
            transport
                .send_line_before(b"never", expired)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        peer.set_nonblocking(true).unwrap();
        let mut byte = [0];
        use std::io::Read;
        assert_eq!(
            peer.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        // Preflight expiry leaves a connection usable: no bytes were sent.
        transport.send_line(b"request").unwrap();
        peer.write_all(b"complete\n").unwrap();
        assert_eq!(
            transport.receive_line_before(expired).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(transport.receive_line().is_err());
        assert!(transport.send_line(b"must-not-replay").is_err());
        transport.reconnect_same_endpoint().unwrap();
        let (_fresh, _) = listener.accept().unwrap();
        assert_eq!(
            transport
                .send_line_before(b"no-reconnect", expired)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let (mut client, mut other) = UnixStream::pair().unwrap();
        assert_eq!(
            write_frame_before(&mut client, b"never", expired)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        other.write_all(b"buffered\n").unwrap();
        let mut reader = BufReader::new(client);
        reader.fill_buf().unwrap();
        assert_eq!(
            read_frame_before(&mut reader, 8192, expired)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(read_frame(&mut reader, 8192).unwrap(), b"buffered");
    }

    #[test]
    fn caller_deadline_is_shared_through_reconnect_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut transport = Transport::connect_before(&path, deadline).unwrap();
        let (mut first, _) = listener.accept().unwrap();
        transport.send_line_before(b"first", deadline).unwrap();
        first.write_all(b"ok\n").unwrap();
        assert_eq!(transport.receive_line_before(deadline).unwrap(), b"ok");
        transport.send_line_before(b"second", deadline).unwrap();
        let (_second, _) = listener.accept().unwrap();
        assert_eq!(
            transport.receive_line_before(deadline).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(Instant::now() >= deadline);
        assert_eq!(
            transport
                .reconnect_same_endpoint_before(deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(transport.send_line(b"no-replay").is_err());
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn caller_caps_trickle_read_and_backpressure_write() {
        let (read, mut write) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            for _ in 0..8 {
                if write.write_all(b"x").is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            // This would complete within the local 500 ms cap if the caller
            // deadline were ignored or renewed by each trickled byte.
            let _ = write.write_all(b"\n");
        });
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut reader = BufReader::new(read);
        assert_eq!(
            read_frame_before(&mut reader, 8192, deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(Instant::now() >= deadline);
        drop(reader);
        writer.join().unwrap();
        let (mut client, _peer) = UnixStream::pair().unwrap();
        let deadline = Instant::now() + Duration::from_millis(150);
        assert_eq!(
            write_frame_before(&mut client, &vec![b'x'; MAX_FRAME_BYTES], deadline)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(Instant::now() >= deadline);
    }

    #[test]
    fn endpoint_replacement_is_rejected_by_deadline_api() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut transport = Transport::connect_before(&path, deadline).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        transport.send_line_before(b"request", deadline).unwrap();
        peer.write_all(b"ok\n").unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        replacement.set_nonblocking(true).unwrap();
        assert!(transport.receive_line_before(deadline).is_err());
        assert!(transport.send_line_before(b"never", deadline).is_err());
        assert!(transport.reconnect_same_endpoint_before(deadline).is_err());
        assert_eq!(
            replacement.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn incomplete_response_requires_explicit_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        transport.send_line(b"request").unwrap();
        peer.write_all(b"partial").unwrap();
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(
            transport.receive_line().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert!(transport.send_line(b"must-not-replay").is_err());
        assert!(transport.check_connection().is_err());
        transport.reconnect_same_endpoint().unwrap();
        let (mut fresh_peer, _) = listener.accept().unwrap();
        transport.send_line(b"fresh").unwrap();
        let mut request = String::new();
        BufReader::new(fresh_peer.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        assert_eq!(request, "fresh\n");
        fresh_peer.write_all(b"ok\n").unwrap();
        assert_eq!(transport.receive_line().unwrap(), b"ok");
    }
    #[test]
    fn reconnects_only_to_unchanged_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut transport = Transport::connect(&path).unwrap();
        let (peer, _) = listener.accept().unwrap();
        drop(peer);
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut disconnected = false;
        while Instant::now() < deadline {
            if transport.check_connection().is_err() {
                disconnected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            disconnected,
            "socket did not observe peer EOF before deadline"
        );
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
    fn missing_endpoint_is_retryable_but_replacement_is_observed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let hidden = dir.path().join("hidden.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let transport = Transport::connect(&path).unwrap();
        let _peer = listener.accept().unwrap();
        fs::rename(&path, &hidden).unwrap();
        assert!(!transport.endpoint_replaced());
        fs::rename(&hidden, &path).unwrap();
        assert!(!transport.endpoint_replaced());
        fs::remove_file(&path).unwrap();
        let _replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(transport.endpoint_replaced());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn full_listen_backlog_never_returns_an_unconnected_stream() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let _queued = UnixStream::connect(&path).unwrap();
        let started = Instant::now();
        let error = connect_stream(&path).expect_err("backlog is still full");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(900));
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
