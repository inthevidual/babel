// Babel for macOS: what the Swift app (macos/) calls in babel-app.
//
// Requests and answers are JSON (UTF-8, NUL-terminated). Strings returned
// are the caller's to free with babel_string_free. Everything may be called
// from the main thread; the listener is called from any thread.

#ifndef BABEL_H
#define BABEL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opens the store in `data_dir` (NULL: Babel's folder in Application
// Support). On failure returns false and sets *error (to free).
bool babel_start(const char *data_dir, char **error);

// Events, as JSON: {"event": "issues" | "external" | "projects" | "sync" |
// "message", ...}. Starts syncing if an account is signed in.
typedef void (*babel_listener)(const char *event_json);
void babel_connect(babel_listener listener);

// Calls `method` with `args` (a JSON object, or NULL); the answer as JSON.
char *babel_call(const char *method, const char *args);

void babel_string_free(char *s);

// A page drawn as RGBA (premultiplied), `width` * `height` * 4 bytes.
typedef struct {
    uint32_t width;
    uint32_t height;
    uint8_t *data;
    size_t len;
} BabelImage;

// Draws page `page` of the translation (side 0) or the original (side 1) of
// the open translation at `scale` pixels per point. False if there is none.
bool babel_render(uint8_t side, uint32_t page, float scale, BabelImage *out);

void babel_image_free(BabelImage *image);

#ifdef __cplusplus
}
#endif

#endif
