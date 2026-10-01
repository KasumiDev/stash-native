# Browsing fixes after f01840a2

Favorites shelves select scenes rated exactly 100/100. A missing rating does not count as a favorite. Card ratings display on a five-star scale (90/100 becomes ★ 4.5). Favorite performer and tag flags retain their separate meanings.

Performer details show Favorites, then alphabetically ordered favorite-tag shelves, then the complete scene grid. Tag details intersect the viewed tag with Favorites and each other favorite tag. Empty shelves are omitted. Each shelf requests 50 scenes at a time and prefetches within its final 12 loaded cards. Replies must match the route, generation, shelf ID, and expected page; appends deduplicate scenes without changing their focus keys. Failed shelf requests retain their page and can retry through the error control or subsequent navigation.

Home hero action edges change slides without leaving the hero. Up moves through adjacent shelves. Home heading clearance follows the measured action row. Collections draw through the full viewport behind navigation, while focus reveal keeps selected cards below the controls. Performer heroes collapse into a pinned name and expand when scrolling back.

Scene Details seats Play or Resume once on fresh metadata arrival. Refreshes and restored entries keep focus. Its title wraps at 50% of screen width on the first line and 65% on the second, with overflow ellipsized. Passive glass tag badges occupy at most two rows, followed by an overflow count; the tag collection remains navigable below.

Scene grids and shelves share the same compositor. Sources outside a 1% tolerance of 16:9 are contained over a blurred still. One outer rounded boundary and focus material cover both layers; preview foregrounds have no inner rounding. Scene blur is decoded off-thread at a maximum 320×180 using the existing shared media budget and a separate still-cache transform. Preview frames remain transient.

## Verification scope

Regression fixtures reproduced the old collection clipping, first-column Up, missing initial Play focus, and hero action-edge escape before fixes. A simulator capture also reproduced the disappearing performer header. Synthetic fixtures cover long titles, missing ratings, portrait/square/wide artwork, empty favorites, intersecting tags, stale replies, and shelves beyond 50 scenes. Simulator captures can establish layout and software shader composition, but do not establish LG cursor behavior, video-plane composition, memory stability, or performance.

TV installation remains user-controlled. After installing the debug package, exercise Left/Right at both hero action edges; Up through several shelves including column one; performer collapse and scroll-back; fresh Scene Details and Back restoration; and a long shelf past its first 50 scenes. Test portrait, square, and wide previews for full-card shine and intact foregrounds, then use pointer/wheel and D-pad transitions. No TV verification is claimed for this change.
