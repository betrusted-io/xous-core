.global arm_cache_flush
arm_cache_flush:
    dsb
    isb

    # Return
    bx lr

.global __aeabi_unwind_cpp_pr0
__aeabi_unwind_cpp_pr0:
  b __aeabi_unwind_cpp_pr0

.global __aeabi_unwind_cpp_pr1
__aeabi_unwind_cpp_pr1:
  b __aeabi_unwind_cpp_pr1
