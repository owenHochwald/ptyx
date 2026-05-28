use ptyx::predict::{EchoPredictor, ReconcileResult};

/// Simulate typing "ls\n" and receiving the correct echo from the server.
#[test]
fn full_cooked_mode_echo_roundtrip() {
    let mut p = EchoPredictor::new(3);

    // predict("ls\n") should produce "ls\r\n"
    let echo = p.predict(b"ls\n").unwrap();
    assert!(
        echo.contains("ls"),
        "predicted echo should contain the typed text"
    );

    // Server echoes exactly what was predicted
    let result = p.reconcile(b"ls\r\n");
    assert!(
        matches!(result, ReconcileResult::Confirmed { .. }),
        "cooked-mode echo should be confirmed, got: {result:?}"
    );
}

/// Prediction should be disabled when max misses is reached.
#[test]
fn prediction_auto_disabled_after_misses() {
    let mut p = EchoPredictor::new(2);
    for _ in 0..2 {
        p.predict(b"a");
        p.reconcile(b"z"); // deliberate mismatch
    }
    assert!(
        !p.enabled,
        "predictor should be disabled after threshold misses"
    );
    assert!(p.predict(b"x").is_none());
}

/// Raw mode output should suppress prediction and clear pending state.
#[test]
fn raw_mode_suppresses_prediction() {
    let mut p = EchoPredictor::new(3);
    p.predict(b"hello"); // pending prediction
    p.check_output_for_raw_mode(b"\x1b[?1049h"); // vim opening alt screen
    assert!(!p.enabled);

    // Output after raw mode — should be Passthrough since pending was cleared
    let result = p.reconcile(b"anything");
    assert!(matches!(result, ReconcileResult::Passthrough));
}

/// Verify that the predictor re-enables after alt-screen exit.
#[test]
fn predictor_re_enables_on_alt_screen_exit() {
    let mut p = EchoPredictor::new(3);
    p.check_output_for_raw_mode(b"\x1b[?1049h"); // enter raw
    assert!(!p.enabled);
    p.check_output_for_raw_mode(b"\x1b[?1049l"); // exit raw
    assert!(p.enabled);
    // Now prediction should work again
    let echo = p.predict(b"hi");
    assert!(echo.is_some());
}
