# Windows setup, simulator, and TV installation

## Installed environment

Use Windows 11 with WSL2 and WSLg. This workspace uses Ubuntu 22.04. Rust stable builds the simulator; Rust nightly with `rust-src` and Clippy runs checks and builds the ARM static library. You do not need to learn C or install a Windows Rust compiler to run this path.

From PowerShell in the repository:

```powershell
wsl --install -d Ubuntu-22.04 --no-launch
powershell -ExecutionPolicy Bypass -File tools/sim.ps1 setup
```

The setup action installs SDL2, SDL2_ttf, OpenGL, C/C++ build tools, NASM, and Rust inside WSL. Cargo caches, FFmpeg build work, runtime state, and staged simulator assets live on the Linux filesystem. The Windows checkout remains the source directory. The launcher checks that WSLg uses hardware graphics acceleration.

## Connect to Stash

Run the app and enter your server URL and optional API key in Settings. Connection Test performs a read-only GraphQL query before saving. Supply either the server base URL or its `/graphql` endpoint. Credentials belong in private runtime configuration, never in source or a commit.

For a command-line override, use your own server address:

```powershell
powershell -ExecutionPolicy Bypass -File tools/sim.ps1 run -ServerUrl http://STASH_HOST:9999/graphql
```

A private JSON file is another option:

```json
{"server_url":"http://STASH_HOST:9999/graphql","api_key":""}
```

Save it as `.stash.local.json` (gitignored), then:

```powershell
powershell -ExecutionPolicy Bypass -File tools/sim.ps1 run -ConfigPath .stash.local.json
powershell -ExecutionPolicy Bypass -File tools/sim.ps1 shot -ConfigPath .stash.local.json
```

Avoid typing real API keys into commands saved by shell history. `-ConfigPath` stages a mode-0600 file in WSL runtime storage. Debug and stable installations use different app directories and identities.

Use arrow keys to move focus, Enter to activate, and Escape to go back. Full hardware video playback requires the TV; host previews decode through the bundled software FFmpeg. The simulator cannot establish LG video-plane composition, firmware compatibility, or TV memory stability.

## Checks

Inside Ubuntu, from `/mnt/d/github/stash_webos`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
make check
CARGO_INCREMENTAL=0 cargo +nightly check --manifest-path rust-modules/Cargo.toml --lib --no-default-features
```

If Ubuntu is running as root (`id -u` prints `0`), run the host gate with `setpriv --bounding-set=-dac_override,-dac_read_search make check`. This removes root's filesystem permission overrides so the write-failure tests exercise normal user permissions.

Run final gates after committing, with a clean tree. The ARM GitHub Actions job uses an ARM64 runner and the upstream webOS cross toolchain. Its downloadable artifact contains the debug `.ipk`; check the verification document for the exact validated commit and build.

## Install the debug package yourself

Download the debug `.ipk` from the verified GitHub Actions artifact and extract its ZIP on Windows. The package ID must be `com.stashnative.app.debug`; its launcher entry is **StashNative debug**. It is a separate installation from Plex and any later stable StashNative build.

In webOS Dev Manager, use your existing rooted-TV connection, choose **Install**, and select the downloaded `.ipk`. Wait for installation to finish, then open **StashNative debug** on the TV and enter your Stash URL and optional key in Settings. This is an app installation; these steps do not install or update TV firmware. Exact webOS 4.4.0 compatibility still needs your device test.

The startup playback fix was tested on your TV, and you confirmed playback works. New cursor, pagination, and animation changes need their own device checks; that playback result does not establish their performance. Start with browsing, focus and Back navigation. For playback/history tests, use only authorized test scenes; do not test counters on content whose records you want to preserve.

For developers using repository TV commands later, read the [TV session skill](../.agents/skills/tv-session/SKILL.md) and acquire the [repository TV lock](../.agents/skills/tv-lock/SKILL.md) only around device operations. Stable publication remains separate.

## Activity and counters

Full playback sends actual watched-time deltas and resume positions, excluding pauses, seeking, buffering, and previews. Additive history requests are serialized and are not automatically repeated after an ambiguous timeout. A playback session records one play after 30 seconds of actual playback. O +1 is an explicit action; favorite editing, metadata editing, and automatic o-count increments are excluded.
