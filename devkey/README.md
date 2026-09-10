# Developer Keys

All images are signed. There is no "unsigned back door" for developers. Instead,
developers are simply handed a key they can use, which when recognized by the
device, triggers a protection mechanism involving wiping security-relevant secrets
(at least on the Baochip-1x; Precursor only changes up the user interface by putting
hash marks across the status bar).

This directory contains a developer key, and an X.509 certificate signed with
the developer key. The key is an Ed25519 key, and you should "kindly note this
is a dev key, don't use for production" (n.b. you can use anything for a private
key in Curve25519, including that very string encoded in base64).

See https://github.com/betrusted-io/betrusted-wiki/wiki/Secure-Boot-and-KEYROM-Layout for
more explanation on why this key exists, and how it may or may not be used.

There is also a dev-pq.key, which is a slh-dsa-128-24 key per NIST SP 800-230-IPD.
The tree cache is also committed here. It is a small blob of 128kiB in size, derived
from dev-pq.key. It greatly accelerates the signing process.

The purpose of the dev-test-pq.key is for testing the special case of an end customer
wishing to *also* replace the devkey. This has to be a fixed, known key because for
test images generated using the "fake" public keys routine, we still need to be able
to sign the image with *something* or else nothing is bootable.