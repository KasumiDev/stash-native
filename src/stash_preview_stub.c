/* Fast host logic tests do not install FFmpeg. Production/simulator builds use stash_preview.c. */
#include "stash_preview.h"
#include <stddef.h>
void *stash_preview_open(const char *directory, void *source, stash_read_fn read, stash_seek_fn seek) {
 (void)directory;(void)source;(void)read;(void)seek;return NULL;
}
int stash_preview_next(void *decoder,uint8_t *rgba,int *width,int *height,int64_t *pts_ms) {
 (void)decoder;(void)rgba;(void)width;(void)height;(void)pts_ms;return -1;
}
void stash_preview_close(void *decoder) {(void)decoder;}
