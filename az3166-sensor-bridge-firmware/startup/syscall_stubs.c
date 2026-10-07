/*
 * -nostartfiles drops crti.o/crtn.o along with crt0.o, which is where
 * _init/_fini normally come from. newlib's __libc_init_array() (called
 * from startup_stm32f412rx.s before main()) calls _init() unconditionally,
 * so something has to provide it. Neither function needs to do anything
 * here — there are no C++ global constructors in this firmware and no
 * other registered init/fini hooks.
 */
void _init(void) {}
void _fini(void) {}
