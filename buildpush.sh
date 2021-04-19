#!/bin/bash

set -e  # exit on any error

UPDATE_FPGA=1
UPDATE_KERNEL=1
UPDATE_LOADER=1
USE_USB=1
FPGA_IMAGE=../betrusted-soc/build/gateware/encrypted.bin
KERNEL_IMAGE=target/riscv32imac-unknown-none-elf/release/xous.img
LOADER_IMAGE=target/riscv32imac-unknown-none-elf/release/loader.bin
CSR_CSV=../betrusted-soc/build/csr.csv.1
USE_IDENTITY=0
USE_NIGHTLY=
IMAGE=hw-image

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
      -l|--loader-skip)
    UPDATE_LOADER=0
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
      --current-csr)
	  CSR_CSV=../betrusted-soc/build/csr.csv
	  shift
	  ;;
      -n|--nightly)
	  USE_NIGHTLY=+nightly
	  shift
	  ;;
      -m|--minimal)
	  IMAGE=minimal
	  shift
	  ;;
      -t|--trngtest)
	  IMAGE=trng-test
	  shift
	  ;;
      -r|--rotest)
	  IMAGE=ro-test
	  shift
	  ;;
      -a|--avtest)
	  IMAGE=av-test
	  shift
	  ;;
      -h|--help)
		echo "$0 provisions betrusted. --kernel-skip skips the kernel, --fpga-skip skips the FPGA. --current-csr indicates to use the CSR for the new FPGA image to do the update (normally you want to use the one corresponding to the older, currently installed version)."
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

# this didn't work. Timestamping is actually kinda broken, because
# you end up capturing just the time that you managed to trigger a
# full rebuild, and not just an incremental rebuild. :-/
#touch services/log-server/src/main.rs # bump the build time in the log server

cargo $USE_NIGHTLY xtask $IMAGE ../betrusted-soc/build/software/soc.svd

# only copy if changed, othrewise it seems to trigger extra build effort...
rsync -a --no-times --checksum ../betrusted-soc/build/software/soc.svd svd2utra/examples/soc.svd
rsync -a --no-times --checksum ../betrusted-soc/build/software/soc.svd emulation/renode.svd

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
    if [ $UPDATE_LOADER -eq 1 ]
    then
      echo "Burning loader"
      sudo wishbone-tool --csr-csv $CSR_CSV --load-name $LOADER_IMAGE --load-address 0x500000 --load-flash
    fi
    if [ $UPDATE_KERNEL -eq 1 ]
    then
      echo "Burning kernel"
      sudo wishbone-tool --csr-csv $CSR_CSV --load-name $KERNEL_IMAGE --load-address 0x980000 --load-flash
    fi
else

  md5sum $FPGA_IMAGE
  md5sum $KERNEL_IMAGE
  md5sum $LOADER_IMAGE
  md5sum $CSR_CSV

  if [ $USE_IDENTITY -eq 1 ]
  then
    # there is a private key
      scp -i $IDENTITY $KERNEL_IMAGE $FPGA_IMAGE $LOADER_IMAGE $DEST_HOST:$DESTDIR/
      scp -i $IDENTITY $CSR_CSV $DEST_HOST:$DESTDIR/soc-csr.csv
  else
      scp $KERNEL_IMAGE $FPGA_IMAGE $LOADER_IMAGE $DEST_HOST:$DESTDIR/
      scp $CSR_CSV $DEST_HOST:$DESTDIR/soc-csr.csv
  fi
fi
