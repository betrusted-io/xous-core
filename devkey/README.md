This directory contains a developer key, and an X.509 certificate signed with
the developer key. The key is an Ed25519 key, and you should "kindly note this
is a dev key, don't use for production" (n.b. you can use anything for a private
key in Curve25519, including that very string encoded in base64).

See https://github.com/betrusted-io/betrusted-wiki/wiki/Secure-Boot-and-KEYROM-Layout for
more explanation on why this key exists, and how it may or may not be used.


