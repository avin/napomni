#[cfg(windows)]
#[test]
#[ignore = "plays the built-in signal for two full loops on the real Windows audio API"]
fn windows_builtin_loop_starts_and_stops_without_restarting_on_reconcile() {
    use napomni::sound::NativeSignal;
    let mut signal = NativeSignal::default();
    signal.reconcile(true).unwrap();
    assert!(signal.active());
    std::thread::sleep(std::time::Duration::from_millis(2100));
    signal.reconcile(true).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2100));
    signal.reconcile(false).unwrap();
    assert!(!signal.active());
    signal.reconcile(false).unwrap();
}
