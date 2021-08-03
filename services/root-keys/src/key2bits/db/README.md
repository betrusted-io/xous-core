# key2bits database files

These are generated from a bespoke prjxray run, targeted at the specific model of FPGA
used in Precursor/Betrusted. Since the hardware will never change, the files are committed
as binary artifacts. For more information, see https://github.com/betrusted-io/rom-locate, and
https://github.com/betrusted-io/prjxray-db will have some leading hints on how to produce
a new DB if parts change (but you'll have to follow a trail of breadcrumbs from there, setup
the entire tool, and do a fuzzing run which takes several days and 100+GiB of disk space...)
