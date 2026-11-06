/* Drives the program with a pointer: moves it, clicks, scrolls.
 *
 * It exists because the tests cannot click. Everything in gitrust that
 * answers a click -- picking a commit, dragging a branch onto another,
 * double-clicking a chip to switch to it -- was checked by reading the code
 * and hoping, which is not checking. With this, and a virtual display, a
 * change to any of it can be run and looked at.
 *
 * Build it, once:
 *
 *     cc -O2 -o tools/poke tools/poke.c -ldl
 *
 * Then, with gitrust on a display of its own:
 *
 *     Xvfb :78 -screen 0 1512x900x24 &
 *     DISPLAY=:78 cargo run --release -- /path/to/a/repository &
 *     tools/poke :78 move 298 127 sleep 400 double 1 sleep 2000
 *     ffmpeg -f x11grab -video_size 1512x900 -i :78 -frames:v 1 shot.png
 *
 *     move X Y      put the pointer there
 *     click N       1 left, 2 middle, 3 right, 4 and 5 the wheel
 *     double N      twice, inside the time a double click is allowed
 *     sleep MS      wait, so the program can answer
 *
 * Several in one go, in the order given.
 *
 * Two things learned the hard way, which cost an hour between them:
 *
 * It needs libXtst and the XTEST extension on the display. Xvfb has it;
 * `xdpyinfo | grep XTEST` says whether yours does.
 *
 * And coordinates go stale. Everything here is pixels, and the window
 * reflows: committing something, or stashing it, takes the uncommitted
 * row out of the graph and moves every row up by one. Take a screenshot
 * with -draw_mouse 1 and look at where the pointer actually landed before
 * believing that a click did nothing.
 *
 * Nothing is linked against: both libraries are opened by name while
 * running, so this builds on a machine with no X headers installed, which
 * is most of them.
 */
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef void *(*open_fn)(const char *);
typedef int (*flush_fn)(void *);
typedef int (*motion_fn)(void *, int, int, int, unsigned long);
typedef int (*button_fn)(void *, unsigned int, int, unsigned long);

static void nap(long ms) {
    struct timespec t = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&t, NULL);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "poke <display> <command...>\n"); return 2; }

    void *x11 = dlopen("libX11.so.6", RTLD_LAZY);
    void *tst = dlopen("libXtst.so.6", RTLD_LAZY);
    if (!x11 || !tst) { fprintf(stderr, "no libX11 or libXtst\n"); return 1; }

    open_fn   xopen  = (open_fn)   dlsym(x11, "XOpenDisplay");
    flush_fn  xflush = (flush_fn)  dlsym(x11, "XFlush");
    motion_fn xmove  = (motion_fn) dlsym(tst, "XTestFakeMotionEvent");
    button_fn xclick = (button_fn) dlsym(tst, "XTestFakeButtonEvent");
    if (!xopen || !xflush || !xmove || !xclick) { fprintf(stderr, "missing symbols\n"); return 1; }

    void *dpy = xopen(argv[1]);
    if (!dpy) { fprintf(stderr, "cannot open %s\n", argv[1]); return 1; }

    for (int i = 2; i < argc; ) {
        const char *what = argv[i++];
        if (!strcmp(what, "move") && i + 1 < argc) {
            int x = atoi(argv[i++]), y = atoi(argv[i++]);
            xmove(dpy, -1, x, y, 0);
        } else if (!strcmp(what, "click") && i < argc) {
            unsigned int b = (unsigned int)atoi(argv[i++]);
            xclick(dpy, b, 1, 0); xflush(dpy); nap(30);
            xclick(dpy, b, 0, 0);
        } else if (!strcmp(what, "double") && i < argc) {
            unsigned int b = (unsigned int)atoi(argv[i++]);
            /* Well inside egui's 300 ms, twice. */
            xclick(dpy, b, 1, 0); xflush(dpy); nap(30);
            xclick(dpy, b, 0, 0); xflush(dpy); nap(60);
            xclick(dpy, b, 1, 0); xflush(dpy); nap(30);
            xclick(dpy, b, 0, 0);
        } else if (!strcmp(what, "sleep") && i < argc) {
            xflush(dpy); nap(atol(argv[i++]));
        } else {
            fprintf(stderr, "do not understand '%s'\n", what); return 2;
        }
        xflush(dpy);
        nap(20);
    }
    return 0;
}
