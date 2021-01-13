#!/bin/sh

CSR_CSV=../betrusted-soc/build/csr.csv

#wishbone-tool -s gdb -s terminal --bind-addr 0.0.0.0 --csr-csv=$CSR_CSV --debug-offset=0xefff0000
wishbone-tool -s gdb --bind-addr 0.0.0.0 --csr-csv=$CSR_CSV --debug-offset=0xefff0000
