#!/bin/bash

set -e  # exit on any error

UPDATE_FPGA=1
UPDATE_KERNEL=1
USE_USB=1
FPGA_IMAGE=../betrusted-soc/build/gateware/encrypted.bin
KERNEL_IMAGE=target/riscv32imac-unknown-none-elf/release/xous.img
CSR_CSV=../betrusted-soc/build/csr.csv.1  # always use the previous version for the current update!
USE_IDENITY=0

POSITIONAL=()
while [[ $# -gt 0 ]]
do
  key="$1"    
  case $key in
      -k|--kernel-skip)
	  UPDATE_KERNEL=0
	  shift
	  ;;
      -f|--fpga-skip)
	  UPDATE_FPGA=0
	  shift
	  ;;
      -c|--copy-to)
	  DEST_HOST=$2
	  USE_USB=0
	  shift
	  shift
	  ;;
      -i|--identity)
	  USE_IDENTITY=1
	  IDENTITY=$2
	  shift
	  shift
	  ;;
      -h|--help)
		echo "$0 provisions betrusted. --kernel-skip skips the kernel, --fpga-skip skips the FPGA."
		echo "Alternatively, using --copy-to <hostname> copies the files to a remote host and skips provisioning."
		echo "  --identity <id_file> supplies an identity file to the scp command."
		echo "  Copying assumes betrusted-scripts repo is cloned on ssh-target at ~/code/betrused-scripts/"
	  exit 0
	  ;;
      *)
	  POSITIONAL+=("$1")
	  shift
	  ;;
  esac
done

set -- "${POSITIONAL[@]}"

DESTDIR=code/precursors

cargo xtask hw-image ../betrusted-soc/build/software/soc.svd

if [ $? -ne 0 ]
then
    echo "build failed, aborting!"
    exit 1
fi

# case of no private key specified
if [ $USE_USB -eq 1 ]
then
    if [ $UPDATE_FPGA -eq 1 ]
    then
      echo "Burning FPGA image"
      sudo wishbone-tool --csr-csv $CSR_CSV --load-name $FPGA_IMAGE --load-address 0x0 --load-flash
      echo "*** Manual power cycle required to reload SoC FPGA configuration ***"
      echo " -> Either issue a power cycle command, or insert paper clip in the hole on the right hand side!"
    fi
    if [ $UPDATE_KERNEL -eq 1 ]
    then
      echo "Burning firmware"
      sudo wishbone-tool --csr-csv $CSR_CSV --load-name $KERNEL_IMAGE --load-address 0x500000 --load-flash
    fi
else
  if [ $USE_IDENTITY -eq 1 ]
  then
    # there is a private key
      scp -i $IDENTITY $KERNEL_IMAGE $FPGA_IMAGE $DEST_HOST:$DESTDIR/
      scp -i $IDENTITY $CSR_CSV $DEST_HOST:$DESTDIR/soc-csr.csv
  else
      scp $KERNEL_IMAGE $FPGA_IMAGE $CSR_CSV $DEST_HOST:$DESTDIR/
      scp $CSR_CSV $DEST_HOST:$DESTDIR/soc-csr.csv
  fi
fi
