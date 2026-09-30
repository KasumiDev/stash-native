#ifndef STASH_PREVIEW_H
#define STASH_PREVIEW_H
#include <stdint.h>
typedef int (*stash_read_fn)(void *, uint8_t *, int);
typedef int64_t (*stash_seek_fn)(void *, int64_t, int);
void *stash_preview_open(const char *directory, void *source, stash_read_fn read, stash_seek_fn seek);
/* 1: converted frame, 2: skipped source frame, 0: loop boundary, -1: failure.
 * Buffer is 640*360*4 bytes. On 2 only pts_ms is updated; RGBA is untouched. */
int stash_preview_next(void *decoder, uint8_t *rgba, int *width, int *height, int64_t *pts_ms);
void stash_preview_close(void *decoder);
#endif
