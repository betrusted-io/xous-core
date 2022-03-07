#include <stdio.h>
#include "libc.h"

int add_one(int a) {
    printf("ffi adding one to %d\n\r", a);
    return a + 1;
}

int malloc_test() {
    int i;

    int randomnumber;
    int size = 32;
    void *p[size];
    for (i = 0; i < size; i++) {
        randomnumber = rand() % 10;
        p[i] = malloc(32 * 32 * randomnumber);
    }

    for (i = size-1; i >= 0; i--) {
        free(p[i]);
    }

    char *foo = malloc(200);
    for (i = 0; i < 200; i++) {
        foo[i] = (char)i;
    }
    char *bar = malloc(200);
    memcpy(bar, foo, 200);
    if (memcmp(foo, bar, 200) != 0) {
        printf("fail on alloc and copy\n");
    } else {
        printf("pass on alloc and copy\n");
    }
    char *baz = realloc(foo, 300);
    int mresult = memcmp(baz, bar, 200);
    if (mresult != 0) {
        printf("fail on realloc copy: %d\n", mresult);
        for (i = 0; i < 200; i++) {
            if (bar[i] != baz[i]) {
                printf("   fail bar[%d](%d) != baz[%d](%d)\n", i, bar[i], i, baz[i]);
            }
        }
    } else {
        printf("pass on realloc copy \n");
    }
    memset(baz, 42, 300);
    int pass = 1;
    for (i = 0; i < 300; i++) {
        if (baz[i] != 42) {
            printf("fail on memset\n");
            pass = 0;
        }
    }
    if (pass == 1) {
        printf("memset passed\n");
    }

    return (0);
}
