#!/usr/bin/env bash

SHORT8=short_8khz.wav
SHORTCD=short_cd.wav
LONG8=long_8khz.wav

UPDATE_SHORT_8=0
UPDATE_SHORT_CD=0
UPDATE_LONG_8=0

for arg in "$@"
do
    case $arg in
	--short8)
	    UPDATE_SHORT_8=1
	    shift
	    ;;
	--shortcd)
	    UPDATE_SHORT_CD=1
	    shift
	    ;;
	--long8)
	    UPDATE_LONG_8=1
	    shift
	    ;;
	-h|--help)
	    echo "$0 provisions audio samples for testing. select which region to update with --short8, --shortcd, --long8"
	    exit 0
	    ;;
	*)
	    OTHER_ARGUMENTS+=("$1")
	    shift
	    ;;
    esac
done

if [ $UPDATE_SHORT_8 -eq 0 ] && [ $UPDATE_SHORT_CD -eq 0 ] && [ $UPDATE_LONG_8 -eq 0 ]
then
    echo "$0 requires one or more arguments of --short8, --shortcd, --long8"
    exit 0
fi

if [ $UPDATE_SHORT_8 -eq 1 ]
then
    md5sum $SHORT8
    sudo wishbone-tool --load-name $SHORT8 --load-address 0x6000000 --load-flash
fi

if [ $UPDATE_SHORT_CD -eq 1 ]
then
    md5sum $SHORTCD
    sudo wishbone-tool --load-name $SHORTCD --load-address 0x6080000 --load-flash
fi

if [ $UPDATE_LONG_8 -eq 1 ]
then
    md5sum $SHORTCD
    sudo wishbone-tool --load-name $LONG8 --load-address 0x6340000 --load-flash
fi
