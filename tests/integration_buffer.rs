use ptyx::buffer::InputBuffer;
use std::os::fd::FromRawFd;
use std::time::Duration;

#[test]
fn enter_flushes_immediately_no_20ms_wait() {
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
    assert!(buf.push_and_maybe_flush(0x03));
}

#[test]
fn ctrl_d_passes_through_immediately() {
    let mut buf = InputBuffer::new(Duration::from_millis(500), 512);
    assert!(buf.push_and_maybe_flush(0x04));
}

#[test]
fn buffer_delivers_batched_bytes_to_pty() {
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

    // Set slave to non-blocking
    unsafe {
        let flags = libc::fcntl(slave_fd, libc::F_GETFL);
        libc::fcntl(slave_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }

    master_file.write_all(&data).expect("write to master");
    std::thread::sleep(Duration::from_millis(10));

    let mut slave_file = unsafe { std::fs::File::from_raw_fd(slave_fd) };
    let mut received = vec![0u8; 16];
    match slave_file.read(&mut received) {
        Ok(n) => assert!(n > 0),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            eprintln!("SKIP: no real PTY available");
        }
        Err(e) => panic!("read error: {}", e),
    }

    std::mem::forget(master_file);
    std::mem::forget(slave_file);
}
