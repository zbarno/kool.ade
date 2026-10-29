#ifndef KOOLADE_QA_ACTION_PROBE_H
#define KOOLADE_QA_ACTION_PROBE_H

#include <X11/Xlib.h>

int qa_action_probe_event(Display *display, Window window, Atom name_atom,
                          Atom utf8_atom, const XEvent *event);

#endif
