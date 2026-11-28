/* Stand-in for libwayland-client.so.0 in the Ultar web bundle.
 *
 * The Servo host links surfman, whose default connection tries Wayland first. The Rust crate
 * wayland-sys loads the library with dlopen and panics when it is missing, instead of letting
 * surfman fall back to the surfaceless Mesa display. The Ultar system has no Wayland compositor,
 * so this library offers every symbol wayland-sys asks for and reports "no display": the connect
 * functions return NULL, everything else is inert. surfman then goes on to X11 (Xlib is missing,
 * also an error) and finally to the surfaceless EGL display of the bundle's Mesa.
 * Pointers are NULL, counts are 0, errors are -1. Built by scripts/bundle-servo-engine.sh. */
#include <stddef.h>
#include <stdint.h>

#define P(name, ...) void *name(__VA_ARGS__) { return NULL; }
#define I(name, ...) int name(__VA_ARGS__) { return -1; }
#define V(name, ...) void name(__VA_ARGS__) { }

P(wl_display_connect_to_fd, int fd)
P(wl_display_connect, const char *name)
V(wl_display_disconnect, void *d)
I(wl_display_get_fd, void *d)
I(wl_display_roundtrip, void *d)
I(wl_display_read_events, void *d)
I(wl_display_prepare_read, void *d)
V(wl_display_cancel_read, void *d)
I(wl_display_dispatch, void *d)
I(wl_display_dispatch_pending, void *d)
V(wl_display_set_max_buffer_size, void *d, size_t n)
I(wl_display_get_error, void *d)
uint32_t wl_display_get_protocol_error(void *d, const void **i, uint32_t *id) { return 0; }
I(wl_display_flush, void *d)
V(wl_event_queue_destroy, void *q)
P(wl_display_create_queue, void *d)
I(wl_display_roundtrip_queue, void *d, void *q)
I(wl_display_prepare_read_queue, void *d, void *q)
I(wl_display_dispatch_queue, void *d, void *q)
I(wl_display_dispatch_queue_pending, void *d, void *q)
P(wl_proxy_create, void *p, const void *i)
V(wl_proxy_destroy, void *p)
I(wl_proxy_add_listener, void *p, void *l, void *d)
P(wl_proxy_get_listener, void *p)
I(wl_proxy_add_dispatcher, void *p, void *f, const void *i, void *d)
P(wl_proxy_marshal_array_constructor, void *p, uint32_t o, void *a, const void *i)
P(wl_proxy_marshal_array_constructor_versioned, void *p, uint32_t o, void *a, const void *i, uint32_t v)
V(wl_proxy_marshal_array, void *p, uint32_t o, void *a)
V(wl_proxy_set_user_data, void *p, void *d)
P(wl_proxy_get_user_data, void *p)
uint32_t wl_proxy_get_id(void *p) { return 0; }
P(wl_proxy_get_class, void *p)
P(wl_proxy_get_display, void *p)
V(wl_proxy_set_queue, void *p, void *q)
uint32_t wl_proxy_get_version(void *p) { return 0; }
P(wl_proxy_create_wrapper, void *p)
V(wl_proxy_wrapper_destroy, void *p)
V(wl_log_set_handler_client, void *f)
V(wl_list_init, void *l)
V(wl_list_insert, void *l, void *e)
V(wl_list_remove, void *e)
I(wl_list_length, const void *l)
I(wl_list_empty, const void *l)
V(wl_list_insert_list, void *l, void *o)
V(wl_array_init, void *a)
V(wl_array_release, void *a)
V(wl_array_add, void *a, size_t n)
V(wl_array_copy, void *a, void *b)
P(wl_proxy_marshal_constructor, void *p, uint32_t o, const void *i)
P(wl_proxy_marshal_constructor_versioned, void *p, uint32_t o, const void *i, uint32_t v)
V(wl_proxy_marshal, void *p, uint32_t o)
