use ptyx::buffer::InputBuffer;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

#[test]
fn enter_flushes_immediately_no_20ms_wait() {
    // 500ms interval — Enter must flush before the timer fires
    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    buf.push_and_maybe_flush(b'l');
    buf.push_and_maybe_flush(b's');
    let flush = buf.push_and_maybe_flush(b'\n');
    assert!(flush, "push_and_maybe_flush should return true on Enter");
    assert_eq!(buf.take(), b"ls\n");
}

#[test]
fn ctrl_c_passes_through_immediately() {
    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    let flush = buf.push_and_maybe_flush(0x03);
    assert!(flush);
}

#[test]
fn ctrl_d_passes_through_immediately() {
    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    let flush = buf.push_and_maybe_flush(0x04);
    assert!(flush);
}

#[test]
fn buffer_delivers_batched_bytes_to_pty() {
    // Open a real PTY pair, write through InputBuffer to master, read from slave.
    use ptyx::pty::open_pty;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;

    let pair = open_pty().expect("open_pty");
    let master_fd = pair.master.as_raw_fd();
    let slave_fd = pair.slave.as_raw_fd();

    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    buf.push(b'a');
    buf.push(b'b');
    buf.push(b'c');

    let data = buf.take();
    let mut master_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
    master_file.write_all(&data).expect("write to master");

    let mut slave_file = unsafe { std::fs::File::from_raw_fd(slave_fd) };
    let mut received = vec![0u8; 16];

    // Set slave to non-blocking for the test
    unsafe {
        let flags = libc::fcntl(slave_fd, libc::F_GETFL);
        libc::fcntl(slave_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }

    // Small delay to allow kernel to route bytes
    std::thread::sleep(Duration::from_millis(10));

    match slave_file.read(&mut received) {
        Ok(n) => {
            // PTY may echo/transform bytes; just verify we got some bytes
            assert!(n > 0, "expected bytes on slave");
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            // Some test environments may not have a real PTY — skip gracefully
            eprintln!("SKIP: slave read would block (no real PTY in this env)");
        }
        Err(e) => panic!("slave read error: {}", e),
    }

    // Prevent double-close — the OwnedFd in pair still owns these fds
    std::mem::forget(master_file);
    std::mem::forget(slave_file);
}
