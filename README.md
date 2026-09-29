# StashNative

A native Stash client for rooted LG webOS televisions, adapted from [PlxNative](https://github.com/GLinnik21/plx-native). Development targets an OLED55C8LLA running webOS 4.4.0. Rust draws the interface through the shared SDL/OpenGL UI; full playback uses the existing LG hardware pipeline.

Source: [KasumiDev/stash-native](https://github.com/KasumiDev/stash-native).

The interface includes Home, Performers, Scenes, Galleries, Tags, Search, and Settings. Home shows newest scenes, favorite performers before other performers ordered by o-count, and favorite tag shelves. Scene cards have delayed focus previews; performer portraits support animated WebP. Galleries support a full-screen viewer and timed slideshows.

See [Windows setup and installation](docs/stashnative-setup.md) for beginner instructions and [verification evidence](docs/stashnative-verification.md) for what has actually been tested. Simulator results do not establish TV playback or performance.

## Build identities

| Flavor | Package ID |
| --- | --- |
| Debug (default) | `com.stashnative.app.debug` |
| Stable | `com.stashnative.app` |
| Nightly | `com.stashnative.app.nightly` |

The executable retains the internal name `plxnative` to preserve the upstream build and diagnostic tooling. Stable publication is a separate task.

## License and credits

This derivative preserves the upstream [GPL license](LICENSE), [third-party notices](THIRD-PARTY-NOTICES.md), fonts, and attribution. The original product description is retained in [the upstream README](docs/upstream-plxnative-readme.md). Historical Plex architecture documents describe the retained engine and are not Stash feature verification.
