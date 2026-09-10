# Stand-alone KAT

This should not be included in the workspace. This tool is provided to test
that the libraries are working correctly in the airgapped enclave used for
keygen & signing. Build it locally, the install it on the airgapped device
and run it to confirm that all of the SHA accelerations are working as
expected. The main concern is that the compiler may be outputting instructions
that are not compatible with the target device because it's a smaller/slower
CPU than the desktop machine used to build & test everything.
