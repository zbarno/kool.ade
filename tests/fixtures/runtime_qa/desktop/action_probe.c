#include <stdio.h>
#include <string.h>

#include <X11/keysym.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>

#include "action_probe.h"

#define WINDOW_PREFIX "Koolade NonRust QA: "

static unsigned int double_clicks = 0;
static unsigned int scroll_events = 0;
static int drag_start_x = -1;
static int drag_start_y = -1;
static int drag_moved = 0;

static void set_action_title(Display *display, Window window, Atom name_atom,
                             Atom utf8_atom, const char *action) {
    char title[sizeof(WINDOW_PREFIX) + 32];
    snprintf(title, sizeof(title), "%s%s", WINDOW_PREFIX, action);
    XStoreName(display, window, title);
    XChangeProperty(display, window, name_atom, utf8_atom, 8, PropModeReplace,
                    (unsigned char *)title, (int)strlen(title));
    XFlush(display);
    printf("[nonrust-fixture] msg=\"action-received\" action=\"%s\"\n", action);
    fflush(stdout);
}

static int inside(int x, int y, int left, int top, int width, int height) {
    return x >= left && x < left + width && y >= top && y < top + height;
}

int qa_action_probe_event(Display *display, Window window, Atom name_atom,
                          Atom utf8_atom, const XEvent *event) {
    if (event->type == ButtonPress && event->xbutton.window == window) {
        if (event->xbutton.button == Button4 || event->xbutton.button == Button5) {
            ++scroll_events;
            if (scroll_events >= 2) {
                set_action_title(display, window, name_atom, utf8_atom, "Scrolled");
            }
            return 1;
        }
        if (event->xbutton.button != Button1) {
            return 0;
        }
        if (inside(event->xbutton.x, event->xbutton.y, 200, 20, 60, 32)) {
            ++double_clicks;
            if (double_clicks >= 2) {
                set_action_title(display, window, name_atom, utf8_atom, "DoubleClick");
            }
            return 1;
        }
        if (inside(event->xbutton.x, event->xbutton.y, 160, 140, 120, 80)) {
            drag_start_x = event->xbutton.x;
            drag_start_y = event->xbutton.y;
            drag_moved = 0;
            return 1;
        }
        return 0;
    }
    if (event->type == MotionNotify && event->xmotion.window == window && drag_start_x >= 0) {
        if (event->xmotion.x != drag_start_x || event->xmotion.y != drag_start_y) {
            drag_moved = 1;
        }
        return 1;
    }
    if (event->type == ButtonRelease && event->xbutton.window == window &&
        event->xbutton.button == Button1 && drag_start_x >= 0) {
        if (drag_moved) {
            set_action_title(display, window, name_atom, utf8_atom, "Dragged");
        }
        drag_start_x = -1;
        drag_start_y = -1;
        drag_moved = 0;
        return 1;
    }
    if (event->type == KeyPress && event->xkey.window == window) {
        KeySym keysym = XLookupKeysym((XKeyEvent *)event, 0);
        Window focus_window = None;
        int focus_mode = 0;
        if (keysym == XK_a && (event->xkey.state & ControlMask) != 0 &&
            XGetInputFocus(display, &focus_window, &focus_mode) && focus_window == window) {
            set_action_title(display, window, name_atom, utf8_atom, "KeyChord");
            return 1;
        }
    }
    return 0;
}
