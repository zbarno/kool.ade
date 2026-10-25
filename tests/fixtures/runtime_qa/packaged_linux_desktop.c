#define _POSIX_C_SOURCE 200809L

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct _XDisplay Display;
typedef unsigned long Window;
typedef unsigned long KeySym;
typedef struct _XGC *GraphicsContext;

typedef struct {
  int type;
  unsigned long serial;
  int send_event;
  Display *display;
  Window window;
  Window root;
  Window subwindow;
  unsigned long time;
  int x;
  int y;
  int root_x;
  int root_y;
  unsigned int state;
  unsigned int keycode;
  int same_screen;
} KeyEvent;

typedef union {
  int type;
  KeyEvent key;
  long padding[24];
} XEvent;

typedef struct {
  void *library;
  Display *(*open_display)(const char *);
  int (*default_screen)(Display *);
  Window (*root_window)(Display *, int);
  Window (*create_simple_window)(Display *, Window, int, int, unsigned int,
                                 unsigned int, unsigned int, unsigned long,
                                 unsigned long);
  int (*store_name)(Display *, Window, const char *);
  int (*select_input)(Display *, Window, long);
  int (*map_window)(Display *, Window);
  GraphicsContext (*create_gc)(Display *, Window, unsigned long, void *);
  unsigned long (*load_font)(Display *, const char *);
  int (*set_font)(Display *, GraphicsContext, unsigned long);
  int (*set_foreground)(Display *, GraphicsContext, unsigned long);
  int (*clear_window)(Display *, Window);
  int (*draw_string)(Display *, Window, GraphicsContext, int, int, const char *,
                     int);
  int (*next_event)(Display *, XEvent *);
  int (*lookup_string)(KeyEvent *, char *, int, KeySym *, void *);
  int (*flush)(Display *);
} X11Api;

static X11Api x11;
static Display *display;
static Window window;
static GraphicsContext graphics;
static char typed_text[128];
static char current_state[160] = "Target:Ready";
static const char app_description[] = "Packaged Linux desktop application";
static const char input_description[] = "Click anywhere, then type a message";

static int load_function(void *library, const char *name, void *output,
                         size_t output_size) {
  void *symbol = dlsym(library, name);
  if (symbol == NULL || output_size != sizeof(symbol)) {
    fprintf(stderr, "could not load X11 function %s\n", name);
    return 0;
  }
  memcpy(output, &symbol, output_size);
  return 1;
}

#define LOAD_FUNCTION(field, symbol)                                           \
  do {                                                                         \
    if (!load_function(x11.library, symbol, &x11.field, sizeof(x11.field))) {   \
      return 1;                                                                \
    }                                                                          \
  } while (0)

static void render(void) {
  x11.store_name(display, window, current_state);
  x11.clear_window(display, window);
  x11.draw_string(display, window, graphics, 32, 42, app_description,
                  (int)(sizeof(app_description) - 1));
  x11.draw_string(display, window, graphics, 32, 78, input_description,
                  (int)(sizeof(input_description) - 1));
  x11.draw_string(display, window, graphics, 32, 132, current_state,
                  (int)strlen(current_state));
  x11.flush(display);
}

int main(int argc, char **argv) {
  (void)argc;
  (void)argv;
  x11.library = dlopen("libX11.so.6", RTLD_NOW);
  if (x11.library == NULL) {
    fprintf(stderr, "the desktop image does not provide libX11.so.6\n");
    return 1;
  }
  LOAD_FUNCTION(open_display, "XOpenDisplay");
  LOAD_FUNCTION(default_screen, "XDefaultScreen");
  LOAD_FUNCTION(root_window, "XRootWindow");
  LOAD_FUNCTION(create_simple_window, "XCreateSimpleWindow");
  LOAD_FUNCTION(store_name, "XStoreName");
  LOAD_FUNCTION(select_input, "XSelectInput");
  LOAD_FUNCTION(map_window, "XMapWindow");
  LOAD_FUNCTION(create_gc, "XCreateGC");
  LOAD_FUNCTION(load_font, "XLoadFont");
  LOAD_FUNCTION(set_font, "XSetFont");
  LOAD_FUNCTION(set_foreground, "XSetForeground");
  LOAD_FUNCTION(clear_window, "XClearWindow");
  LOAD_FUNCTION(draw_string, "XDrawString");
  LOAD_FUNCTION(next_event, "XNextEvent");
  LOAD_FUNCTION(lookup_string, "XLookupString");
  LOAD_FUNCTION(flush, "XFlush");

  display = x11.open_display(NULL);
  if (display == NULL) {
    fprintf(stderr, "the private QA display is unavailable\n");
    return 1;
  }
  int screen = x11.default_screen(display);
  Window root = x11.root_window(display, screen);
  window = x11.create_simple_window(display, root, 0, 0, 640, 360, 1, 0,
                                    0x00f3f4f6);
  x11.select_input(display, window, (1L << 15) | (1L << 2) | 1L);
  graphics = x11.create_gc(display, window, 0, NULL);
  if (graphics == NULL) {
    fprintf(stderr, "could not create the application drawing context\n");
    return 1;
  }
  unsigned long font = x11.load_font(display, "fixed");
  if (font == 0) {
    fprintf(stderr, "the private display does not provide its fixed font\n");
    return 1;
  }
  x11.set_font(display, graphics, font);
  x11.set_foreground(display, graphics, 0x00111827);
  x11.map_window(display, window);
  render();

  for (;;) {
    XEvent event;
    memset(&event, 0, sizeof(event));
    x11.next_event(display, &event);
    if (event.type == 4) {
      strcpy(current_state, "Target:Clicked");
      render();
    } else if (event.type == 2) {
      char bytes[16];
      KeySym keysym = 0;
      int count = x11.lookup_string(&event.key, bytes, (int)sizeof(bytes),
                                    &keysym, NULL);
      size_t used = strlen(typed_text);
      for (int index = 0; index < count && used + 1 < sizeof(typed_text);
           index++) {
        unsigned char character = (unsigned char)bytes[index];
        if (character >= 32 && character <= 126) {
          typed_text[used++] = (char)character;
          typed_text[used] = '\0';
        }
      }
      snprintf(current_state, sizeof(current_state), "Target:Typed:%s",
               typed_text);
      render();
    } else if (event.type == 12) {
      render();
    }
  }
}
