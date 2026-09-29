//! `plxnative-sim` — the desktop UI simulator.
//!
//! The same application core the television runs, linked against a desktop SDL2 and desktop GL
//! instead of LG's. It draws the StashNative interface against a Stash GraphQL server.
//!
//! **What it is for.** The television serializes the entire dev loop: one set, one app instance,
//! and two `tests/run.py` jobs kill each other's app. That makes every UI change a queue. Several
//! simulators can run at once — each with its own instance root (`PLXNATIVE_RUNTIME_DIR`), so each
//! has its own trigger namespace, its own remote FIFO and its own event log — which is what lets
//! independent UI and data-layer work proceed in parallel.
//!
//! **What it is NOT.** Full hardware playback is unavailable; card previews use bundled software decoding. The 29-symbol media seam does not exist off-device, and
//! `player::ffi`'s host arm reports the same "no video path" failure a television with no usable
//! ACB binding reports, so Play lands on the app's real failure read-out. Nor is it a substitute
//! for the device on anything the GPU decides: the `--fps` gates are calibrated to the SM9000's
//! Mali, and text rasterization here goes through a different FreeType. A green run on this binary
//! is evidence about the host. Layout, focus, navigation, and the Stash data layer are
//! the parts that do transfer — and they are most of the UI work.
//!
//! Usage:
//!   plxnative-sim [pms-host] [pms-port]
//! Environment:
//!   STASH_URL / STASH_API_KEY                optional overrides for saved Stash settings
//!   PLXNATIVE_RUNTIME_DIR                     this instance's trigger/FIFO/log root
//!   PLXNATIVE_APP_DIR                         where appfont*.ttf and the icons live (repo `pkg/`)

use plxnative_modules::plex_run;
use std::ffi::CString;
use std::os::raw::c_int;

/// Defaults chosen to fail loudly rather than silently talk to the wrong thing.
const DEFAULT_PORT: u16 = 32400;

fn main() {
    // SIGPIPE needs no handling here, unlike `src/main.c:105`: that file installs `SIG_IGN` by hand
    // precisely because a C `main` skips Rust's `std::rt::init`. This IS a Rust main, so std has
    // already done it, and the first PMS socket closed mid-write cannot kill the process.

    let mut args = std::env::args().skip(1);
    let host = args
        .next()
        .or_else(|| std::env::var("PLXNATIVE_PMS_HOST").ok())
        .unwrap_or_default();
    if host == "--write-synthetic-initial" {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let result = (|| -> Result<(), &'static str> {
            let path = args.next().ok_or("missing output path")?;
            let seed = args.next().and_then(|value| value.parse::<u32>().ok()).ok_or("invalid seed")?;
            let port = args.next().and_then(|value| value.parse::<u16>().ok()).ok_or("invalid port")?;
            let settings = match args.next() {
                None => None,
                Some(value) if value == "flow12" => Some(value),
                Some(value) if value.starts_with("settings=") =>
                    Some(value.trim_start_matches("settings=").to_string()),
                Some(_) => return Err("unsupported synthetic initial argument"),
            };
            if args.next().is_some() { return Err("unexpected argument"); }
            let encoded = plxnative_modules::synthetic_home_initial(seed,port,settings)?;
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
                .open(path).map_err(|_| "cannot create initial input file")?;
            file.write_all(encoded.as_bytes()).map_err(|_| "cannot write initial input file")
        })();
        if let Err(reason) = result { eprintln!("synthetic initial input: {reason}"); std::process::exit(2); }
        return;
    }
    let port: u16 = args
        .next()
        .or_else(|| std::env::var("PLXNATIVE_PMS_PORT").ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    // The instance root must exist before anything writes into it, and the event log is truncated
    // per launch — `src/main.c` does the same on the television, and `tests/run.py` relies on the
    // log starting empty to date its first line.
    let root = plxnative_modules::sim_runtime_dir();
    if let Err(e) = std::fs::create_dir_all(&root) {
        eprintln!(
            "plxnative-sim: cannot create runtime dir {}: {e}",
            root.display()
        );
        std::process::exit(1);
    }
    let events = plxnative_modules::sim_events_log();
    // The banner is the FIRST line of the log, so a log read from the middle, tailed, or pasted
    // into an issue still declares what produced it. Every heartbeat additionally carries `sim=1`
    // (see `app.rs`'s SIM_TAG) — one marker at the top is easy to scroll past.
    let banner = "sim: THIS IS THE DESKTOP SIMULATOR, not a television. No video pipeline; \
                  frame rates here describe the host GPU and must never be read as an fps gate.\n";
    if let Err(e) = std::fs::write(&events, banner) {
        eprintln!("plxnative-sim: cannot truncate {}: {e}", events.display());
        std::process::exit(1);
    }

    eprintln!(
        "stashnative-sim: runtime={} log={}",
        root.display(),
        events.display()
    );

    let c_host = CString::new(host).unwrap_or_else(|_| {
        eprintln!("plxnative-sim: host contains a NUL byte");
        std::process::exit(2)
    });

    // `as c_int` directly — an intermediate i16 would wrap every port above 32767.
    let rc = plex_run(c_host.as_ptr(), port as c_int);
    exit_without_atexit_teardown(rc)
}

/// Leave the process WITHOUT running libc's `atexit` handlers or shared-library destructors.
///
/// `plex_run` returns with detached workers still alive — the sign-in PIN request, the poster
/// fetchers, anything else parked in libcurl — because nothing in the app joins them, and on the
/// television nothing has to. `std::process::exit` would call libc `exit`, whose handlers include
/// OpenSSL 3's `OPENSSL_cleanup`: it frees libcrypto's global tables underneath a worker that is
/// mid-handshake, and the simulator dies with SIGSEGV after it has already done its job. That is
/// what failed the Linux simulator CI launch intermittently (core dump: main thread in
/// `OPENSSL_cleanup` from `exit`, worker in `X509_STORE_load_file_ex` under `auth::mint_pin`).
///
/// Nothing that must survive the process lives in those handlers: the event log is appended
/// unbuffered, the app's own shutdown (telemetry flush included) already ran inside `plex_run`,
/// and the two std streams are flushed here by hand.
fn exit_without_atexit_teardown(rc: c_int) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `_exit` is async-signal-safe and takes no pointers; it ends every thread at once,
    // which is exactly the point.
    unsafe { libc::_exit(rc) }
}
