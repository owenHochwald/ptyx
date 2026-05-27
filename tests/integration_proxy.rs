use ptyx::proxy::PtyProxy;

#[test]
fn pty_proxy_type_is_sized() {
    let _ = std::mem::size_of::<PtyProxy>();
}
