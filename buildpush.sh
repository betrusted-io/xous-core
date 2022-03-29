#!/usr/bin/env bash

set -e  # exit on any error

UPDATE_FPGA=1
UPDATE_KERNEL=1
UPDATE_LOADER=1
USE_USB=1
FPGA_IMAGE=precursors/soc_csr.bin
KERNEL_IMAGE=target/riscv32imac-unknown-xous-elf/release/xous.img
LOADER_IMAGE=target/riscv32imac-unknown-xous-elf/release/loader.bin
CSR_CSV=
USE_IDENTITY=0
USE_NIGHTLY=
IMAGE=hw-image
SOC_SVD=precursors/soc.svd

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
	  CSR_CSV="--csr-csv ../betrusted-soc/build/csr.csv"
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
      --image)
	  IMAGE=$2
	  shift
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

cargo $USE_NIGHTLY xtask $IMAGE $SOC_SVD

if [ $? -ne 0 ]
then
    echo "build failed, aborting!"
    exit 1
fi

# case of no private key specified
if [ $USE_USB -eq 1 ]
then
    if [ $UPDATE_LOADER -eq 1 ]
    then
      echo "Burning loader"
      cd tools && ./usb_update.py -l
    fi
    if [ $UPDATE_KERNEL -eq 1 ]
    then
      echo "Burning kernel"
      cd tools && ./usb_update.py -k
    fi
    if [ $UPDATE_FPGA -eq 1 ]
    then
	echo "Burning FPGA image"
	cd tools && ./usb_update.py -s
	echo "*** Select 'Install gateware update' from the main menu to apply the update with your root keys ***"
    fi
else

  if [ -n "$OVERRIDE_SVD" ]
  then
      FPGA_IMAGE=precursors/soc_csr.bin
  fi
  if [ -e "$FPGA_IMAGE" ]
  then
      md5sum $FPGA_IMAGE
  fi
  md5sum $KERNEL_IMAGE
  md5sum $LOADER_IMAGE

  if [ $USE_IDENTITY -eq 1 ]
  then
      # there is a private key
      echo "Copying to $DEST_HOST:$DESTDIR/ with public key $IDENTITY"
      scp -i $IDENTITY $KERNEL_IMAGE $FPGA_IMAGE $LOADER_IMAGE $DEST_HOST:$DESTDIR/
  else
      echo "Copying to $DEST_HOST:$DESTDIR/ without public key"
      scp $KERNEL_IMAGE $FPGA_IMAGE $LOADER_IMAGE $DEST_HOST:$DESTDIR/
  fi
fi
