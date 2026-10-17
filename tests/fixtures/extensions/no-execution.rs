// Adversarial installation fixture: any execution creates an observable marker.
// This deliberately does not implement the extension protocol.
fn main() {
    std::fs::write("/tmp/save-extension-must-not-execute", b"unexpected execution\n").unwrap();
}
