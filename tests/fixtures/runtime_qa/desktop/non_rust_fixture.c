/*
 * Non-Rust packaged executable fixture for the isolated Linux desktop QA
 * driver (tech-stack neutrality amendment of GitHub issue #167).
 *
 * Compiled C/Xlib: no window manager assumed, no toolkit beyond libX11.
 * Behaviors under test by the rootless scenario:
 *   - one real top-level window whose state is carried in _NET_WM_NAME;
 *   - click region A (approx 100,36): Ready -> Clicked;
 *   - click region B (approx 100,88): Clicked -> Armed (typing allowed);
 *   - keyboard input is honored only while this window holds X input
 *     focus (models applications whose editors require focus; the QA
 *     driver must focus the window before interacting, or the keys are
 *     dropped and the Typed:ok expectation can never be met);
 *   - ConfigureNotify tracks driver-initiated resizes;
 *   - structured log lines on stdout;
 *   - clean SIGTERM/SIGHUP shutdown;
 *   - click region C (approx 100,136): simulated fatal abort (exit 1)
 *     exercising driver crash detection.
 * Built by tests/fixtures/runtime_qa/desktop/build.sh; never committed
 * as a binary.
 */
#include <ctype.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <time.h>
#include <unistd.h>

#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#ifdef KOOLADE_QA_ABSENT_DEP
/* Provided only by the link-time stub for the missing-dependency variant;
 * the reference stamps a NEEDED entry the loader must fail to resolve. */
extern uint32_t koolade_qa_absent_dep_marker(void);
#endif

#define WINDOW_PREFIX "Koolade NonRust QA: "
#define BUFFER_CAPACITY 64

typedef enum { StateReady, StateClicked, StateArmed } FixtureState;

typedef struct {
    int x_origin;
    int y_origin;
    int width;
    int height;
} RegionBox;

static volatile sig_atomic_t g_shutdown = 0;
static Display *g_display = NULL;
static Atom g_net_wm_name = None;
static Atom g_utf8_string = None;
static char g_buffer[BUFFER_CAPACITY + 1];

static void on_shutdown(int signum) {
    (void)signum;
    g_shutdown = 1;
}

static void log_kv(const char *msg, const char *kv) {
    if (kv == NULL || kv[0] == '\0') {
        printf("[nonrust-fixture] msg=\"%s\"\n", msg);
    } else {
        printf("[nonrust-fixture] msg=\"%s\" %s\n", msg, kv);
    }
    fflush(stdout);
}

static void set_title(Display *display, Window window, const FixtureState *state) {
    char title[sizeof(WINDOW_PREFIX) + sizeof("Typed:") + BUFFER_CAPACITY];
    const char *suffix = "Ready";
    switch (*state) {
        case StateClicked:
            suffix = "Clicked";
            break;
        case StateArmed:
            if (g_buffer[0] != '\0') {
                snprintf(title, sizeof(title), "%sTyped:%s", WINDOW_PREFIX, g_buffer);
                goto store;
            }
            suffix = "Armed";
            break;
        default:
            break;
    }
    snprintf(title, sizeof(title), "%s%s", WINDOW_PREFIX, suffix);
store:
    XStoreName(display, window, title);
    XChangeProperty(display, window, g_net_wm_name, g_utf8_string, 8, PropModeReplace,
                    (unsigned char *)title, (int)strlen(title));
    XFlush(display);
    log_kv("renamed", NULL);
}

static int in_region(const RegionBox *region, int x, int y) {
    return x >= region->x_origin && x < region->x_origin + region->width &&
           y >= region->y_origin && y < region->y_origin + region->height;
}

static void redraw(Window window, int width, int height, const FixtureState *state,
                   GC graphics) {
    XClearArea(g_display, window, 0, 0, width, height, True);
    const RegionBox regions[3] = {
        {80, 20, 60, 32},
        {80, 70, 60, 32},
        {80, 120, 60, 32},
    };
    const char *labels[3] = {"A: activate", "B: arm typing", "C: fatal click"};
    for (int index = 0; index < 3; ++index) {
        XDrawRectangle(g_display, window, graphics, regions[index].x_origin,
                       regions[index].y_origin, (unsigned int)regions[index].width,
                       (unsigned int)regions[index].height);
        XDrawString(g_display, window, graphics, 160,
                    regions[index].y_origin + 20, labels[index],
                    (int)strlen(labels[index]));
    }
    char banner[32];
    const char *banner_suffix = "Ready";
    if (*state == StateClicked) {
        banner_suffix = "Clicked";
    } else if (*state == StateArmed) {
        banner_suffix = "Armed";
    }
    snprintf(banner, sizeof(banner), "STATE: %s", banner_suffix);
    XDrawString(g_display, window, graphics, 16, height - 12, banner,
                (int)strlen(banner));
}

static void handle_button(const XEvent *event, Window window, int width, int height,
                          FixtureState *state, GC graphics) {
    if (event->xbutton.button != Button1) {
        return;
    }
    const RegionBox regions[3] = {
        {80, 20, 60, 32},
        {80, 70, 60, 32},
        {80, 120, 60, 32},
    };
    char detail[96];
    if (in_region(&regions[0], event->xbutton.x, event->xbutton.y)) {
        snprintf(detail, sizeof(detail), "region=A");
        log_kv("click", detail);
        if (*state == StateReady) {
            *state = StateClicked;
            g_buffer[0] = '\0';
            set_title(g_display, window, state);
            redraw(window, width, height, state, graphics);
        }
        return;
    }
    if (in_region(&regions[1], event->xbutton.x, event->xbutton.y)) {
        snprintf(detail, sizeof(detail), "region=B");
        log_kv("click", detail);
        if (*state == StateClicked) {
            *state = StateArmed;
            g_buffer[0] = '\0';
            set_title(g_display, window, state);
            redraw(window, width, height, state, graphics);
        }
        return;
    }
    if (in_region(&regions[2], event->xbutton.x, event->xbutton.y)) {
        log_kv("click", "region=C");
        log_kv("critical", "reason=simulated_fatal_abort");
        XFlush(g_display);
        struct timespec delay = {0, 50 * 1000 * 1000};
        nanosleep(&delay, NULL);
        exit(1);
    }
    log_kv("click", "outside tracked regions");
}

static void handle_key(const XEvent *event, Window window, const FixtureState *state,
                       int width, int height, GC graphics) {
    if (*state != StateArmed) {
        log_kv("drop-key", "reason=not_armed");
        return;
    }
    Window focus_window;
    int focus_mode;
    if (!XGetInputFocus(g_display, &focus_window, &focus_mode)) {
        return;
    }
    if (focus_window != window) {
        /* Pointer events are not gated by input focus; keys are. This is
         * the observable that pins the driver's focus-before-input
         * behavior for packaged targets without a window manager. */
        log_kv("drop-key", "reason=no_input_focus");
        return;
    }
    char character[8] = {0};
    KeySym keysym = 0;
    XLookupString((XKeyEvent *)event, character, sizeof(character) - 1, &keysym, NULL);
    if (character[0] == '\0' || !isprint((unsigned char)character[0])) {
        return;
    }
    if (strlen(g_buffer) >= BUFFER_CAPACITY) {
        log_kv("drop-key", "reason=buffer_full");
        return;
    }
    strncat(g_buffer, character, BUFFER_CAPACITY - strlen(g_buffer));
    g_buffer[BUFFER_CAPACITY] = '\0';
    char detail[80];
    snprintf(detail, sizeof(detail), "buffer_length=%zu", strlen(g_buffer));
    log_kv("typed", detail);
    set_title(g_display, window, state);
    redraw(window, width, height, state, graphics);
}

int main(int argc, char **argv) {
    if (argc < 2 || strcmp(argv[1], "--qa-fixture") != 0) {
        fprintf(stderr, "usage: %s --qa-fixture\n", argv[0]);
        return 2;
    }
#ifdef KOOLADE_QA_ABSENT_DEP
    (void)koolade_qa_absent_dep_marker();
#endif
    struct sigaction shutdown_action = {.sa_handler = on_shutdown};
    sigemptyset(&shutdown_action.sa_mask);
    sigaction(SIGTERM, &shutdown_action, NULL);
    sigaction(SIGHUP, &shutdown_action, NULL);
    setvbuf(stdout, NULL, _IOLBF, 0);

    g_display = XOpenDisplay(NULL);
    if (g_display == NULL) {
        log_kv("error", "reason=x_open_display_failed");
        return 3;
    }
    log_kv("booted", "argv=--qa-fixture");
    const int screen_index = DefaultScreen(g_display);
    g_net_wm_name = XInternAtom(g_display, "_NET_WM_NAME", False);
    g_utf8_string = XInternAtom(g_display, "UTF8_STRING", False);
    const unsigned int width = 480;
    const unsigned int height = 320;
    XSetWindowAttributes attributes = {
        .background_pixel = BlackPixel(g_display, screen_index),
        .border_pixel = WhitePixel(g_display, screen_index),
        .event_mask = ExposureMask | ButtonPressMask | KeyPressMask |
                      StructureNotifyMask,
    };
    const Window window =
        XCreateWindow(g_display, RootWindow(g_display, screen_index), 0, 0, width, height,
                      0, DefaultDepth(g_display, screen_index), InputOutput,
                      DefaultVisual(g_display, screen_index), CWBackPixel | CWBorderPixel |
                              CWEventMask,
                      &attributes);
    Font font = XLoadFont(g_display, "fixed");
    GC graphics = None;
    if (font != None) {
        graphics = XCreateGC(g_display, window, 0, NULL);
        XSetFont(g_display, graphics, font);
        XSetForeground(g_display, graphics, WhitePixel(g_display, screen_index));
    } else {
        log_kv("warning", "reason=core_font_unavailable");
    }
    FixtureState state = StateReady;
    set_title(g_display, window, &state);
    XMapWindow(g_display, window);
    XFlush(g_display);

    int width_observed = (int)width;
    int height_observed = (int)height;
    int mapped_logged = 0;
    int interrupted = 0;
    while (!interrupted) {
        if (g_shutdown) {
            log_kv("shutdown", "reason=termin_signal");
            break;
        }
        fd_set readable;
        FD_ZERO(&readable);
        FD_SET(ConnectionNumber(g_display), &readable);
        struct timeval timeout = {0, 50 * 1000};
        const int ready = select(ConnectionNumber(g_display) + 1, &readable, NULL, NULL,
                                 &timeout);
        if (ready > 0) {
            while (XPending(g_display)) {
                XEvent event;
                XNextEvent(g_display, &event);
                switch (event.type) {
                    case MappingNotify:
                        XRefreshKeyboardMapping((XMappingEvent *)&event);
                        break;
                    case MapNotify:
                        if (event.xmap.window == window && !mapped_logged) {
                            mapped_logged = 1;
                            char detail[64];
                            snprintf(detail, sizeof(detail), "window=%lu",
                                     (unsigned long)window);
                            log_kv("mapped", detail);
                        }
                        break;
                    case Expose:
                        if (graphics != None) {
                            redraw(window, width_observed, height_observed, &state, graphics);
                        }
                        break;
                    case ConfigureNotify:
                        if (event.xconfigure.window == window &&
                            (event.xconfigure.width != width_observed ||
                             event.xconfigure.height != height_observed)) {
                            width_observed = event.xconfigure.width;
                            height_observed = event.xconfigure.height;
                            char detail[64];
                            snprintf(detail, sizeof(detail),
                                     "width=%d height=%d", width_observed, height_observed);
                            log_kv("resized", detail);
                            if (graphics != None) {
                                redraw(window, width_observed, height_observed, &state,
                                       graphics);
                            }
                        }
                        break;
                    case ButtonPress:
                        if (event.xbutton.window == window) {
                            handle_button(&event, window, width_observed, height_observed,
                                          &state, graphics);
                        }
                        break;
                    case KeyPress:
                        if (event.xkey.window == window) {
                            handle_key(&event, window, &state, width_observed,
                                       height_observed, graphics);
                        }
                        break;
                    default:
                        break;
                }
            }
        }
    }
    if (graphics != None) {
        XFreeGC(g_display, graphics);
    }
    if (font != None) {
        XUnloadFont(g_display, font);
    }
    XCloseDisplay(g_display);
    return 0;
}
