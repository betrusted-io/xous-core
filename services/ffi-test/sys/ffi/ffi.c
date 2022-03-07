#include <stdio.h>
#include "libc.h"

int add_one(int a) {
    printf("ffi adding one to %d\n\r", a);
    return a + 1;
}

/* returns an array of arrays of char*, all of which NULL */
char ***alloc_matrix(unsigned rows, unsigned columns) {
    char ***matrix = malloc(rows * sizeof(char **));
    unsigned row = 0;
    unsigned column = 0;
    if (!matrix) return NULL;

    for (row = 0; row < rows; row++) {
        matrix[row] = calloc(columns, sizeof(char *));
        if (!matrix[row]) return NULL;
        for (column = 0; column < columns; column++) {
            matrix[row][column] = NULL;
        }
    }
    return matrix;
}

/* deallocates an array of arrays of char*, calling free() on each */
void free_matrix(char ***matrix, unsigned rows, unsigned columns) {
    unsigned row = 0;
    unsigned column = 0;
    for (row = 0; row < rows; row++) {
        for (column = 0; column < columns; column++) {
            printf("column %d row %d\n", column, row);
            free(matrix[row][column]);
        }
        free(matrix[row]);
    }
    free(matrix);
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
    /*
    int x = 6;
    char *** matrix = alloc_matrix(x, x);
    if (matrix == NULL) return 1;
    free_matrix(matrix, x, x);
    */
    return (0);
}
