// Exercise Swift's exported entry points from a native main(), as used by the
// Rust executable before AppKit starts its event loop. No windows are opened.
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>

typedef void (*Completion)(uint64_t, const uint8_t *, size_t);
typedef void (*Submit)(uint64_t, const uint8_t *, size_t, Completion);
typedef void (*Cancel)(uint64_t);
extern void loom_app_intents_install(Submit, Cancel);
extern void loom_app_intents_shutdown(void);
static void submit(uint64_t id, const uint8_t *bytes, size_t size, Completion completion) {
    (void)id; (void)bytes; (void)size; (void)completion;
}
static void cancel(uint64_t id) { (void)id; }
int main(void) {
    loom_app_intents_install(submit, cancel);
    loom_app_intents_shutdown();
    puts("App Intents: C ABI install/shutdown from native main passed");
    return 0;
}
