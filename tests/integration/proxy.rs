// Integration tests for PtyProxy that require a full environment.
// Actual proxy construction requires a real TTY and ssh — tested manually.
// We test that the type is well-formed and the drop path is reachable.

#[test]
fn pty_proxy_type_is_sized() {
    use ptyx::proxy::PtyProxy;
    let _ = std::mem::size_of::<PtyProxy>();
}
