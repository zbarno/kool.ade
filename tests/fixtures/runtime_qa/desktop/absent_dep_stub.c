/* Link-time stub for the missing-dependency variant of the non-Rust
 * desktop QA fixture. Linked as a shared object whose SONAME is never
 * shipped with the artifact, forcing the dynamic linker to fail at
 * exec time (the approved-artifact flow must classify this as missing
 * dependencies rather than treating the target as launched). */
#include <stdint.h>

uint32_t koolade_qa_absent_dep_marker(void) {
    return 0xc001u;
}
