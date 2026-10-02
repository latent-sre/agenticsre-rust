//! Bounded local stdio adapter. The owned loop is the sole protocol and worker lifetime owner.
mod grants;
#[cfg(target_os = "linux")]
mod io;
mod mapping;
mod protocol;
#[cfg(target_os = "linux")]
mod server;
pub use grants::Options;

/// Serve until input closes or a bounded lifecycle failure occurs. No operation is retried.
pub fn serve(options: Options) -> u8 {
    #[cfg(target_os = "linux")]
    {
        server::serve(options)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = options;
        2
    }
}

/// Fixed, best-effort metadata only; never blocking or caller-controlled diagnostic text.
pub fn invalid_startup() {
    #[cfg(target_os = "linux")]
    io::diagnostic(b"{\"event\":\"mcp_startup_refused\"}\n");
}
