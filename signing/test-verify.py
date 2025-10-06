#!/usr/bin/env python3
import base64
import hashlib
import argparse
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.exceptions import InvalidSignature

def verify_signature(file_path: Path, pubkey_b64: str, sig_b64: str, authdata_b64: str):
    pubkey = base64.b64decode(pubkey_b64)
    signature = base64.b64decode(sig_b64)
    authdata = base64.b64decode(authdata_b64)

    client_data = file_path.read_bytes()
    client_data_hash = hashlib.sha256(client_data).digest()
    message = authdata + client_data_hash

    public_key = ed25519.Ed25519PublicKey.from_public_bytes(pubkey)
    try:
        public_key.verify(signature, message)
        print("Signature is valid.")
    except InvalidSignature:
        print("Signature is INVALID.")

def main():
    parser = argparse.ArgumentParser(
        description="Verify FIDO2 Ed25519-SK signature for a given clientDataJSON file."
    )
    parser.add_argument("file", type=Path, help="Path to clientDataJSON file")
    parser.add_argument("pubkey_b64", help="Base64-encoded 32-byte Ed25519 public key")
    parser.add_argument("sig_b64", help="Base64-encoded 64-byte signature")
    parser.add_argument("authdata_b64", help="Base64-encoded 37-byte authenticator data")

    args = parser.parse_args()
    verify_signature(args.file, args.pubkey_b64, args.sig_b64, args.authdata_b64)

if __name__ == "__main__":
    main()

# test data is 64 bytes of 0, signed with beta-key
#
# pubkey gJeZKe3QTkASS1LK6a5Uskvf9yp7igBMQQZb0UAgeKc=
# authdata 4wYQ6KFiEVlg/h7CI+ZSnJ9LboAgDcteXDIcivHisb8FAAACDw==
# signature uYiL6JV0h4LcZ95F3i4o09Lf29zI7PAM11LGfIolkOiJooeOUDm2tOIk2u/OuaiZgN62UZ4a6coNSwZkV9UxCg==