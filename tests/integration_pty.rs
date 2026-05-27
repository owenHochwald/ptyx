use ptyx::pty::{get_pty_size, open_pty, set_pty_size};
use std::os::unix::io::AsRawFd;

#[test]
fn open_pty_returns_valid_fds() {
    let pair = open_pty().expect("open_pty");
    assert!(pair.master.as_raw_fd() > 0);
    assert!(pair.slave.as_raw_fd() > 0);
}

#[test]
fn pty_size_set_and_get_round_trips() {
    let pair = open_pty().expect("open_pty");
    let fd = pair.master.as_raw_fd();
    set_pty_size(fd, 24, 80).expect("set_pty_size");
    let (rows, cols) = get_pty_size(fd).expect("get_pty_size");
    assert_eq!(rows, 24);
    assert_eq!(cols, 80);
}

#[test]
fn open_pty_gives_distinct_fds() {
    let pair = open_pty().expect("open_pty");
    assert_ne!(pair.master.as_raw_fd(), pair.slave.as_raw_fd());
}
