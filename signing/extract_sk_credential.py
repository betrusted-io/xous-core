#!/usr/bin/env python3
"""
Extract FIDO/U2F credential id (key_handle) from an OpenSSH sk private key.

Usage:
  python3 extract_sk_credential.py /path/to/id_ed25519_sk
  cat /path/to/id_ed25519_sk | python3 extract_sk_credential.py -
"""
import sys, argparse, base64, struct
from pathlib import Path

def read_u32(buf, off):
    if off+4 > len(buf):
        raise ValueError("unexpected end while reading u32")
    return struct.unpack(">I", buf[off:off+4])[0], off+4

def read_string(buf, off):
    l, off = read_u32(buf, off)
    if off + l > len(buf):
        raise ValueError("unexpected end while reading string")
    return buf[off:off+l], off + l

def read_byte(buf, off):
    if off >= len(buf):
        raise ValueError("unexpected end while reading byte")
    return buf[off], off+1

def extract_key_info(priv_blob):
    # priv_blob layout: two checkints, then for each key:
    #   string keytype
    #   string pubkey_blob
    #   string privkey_blob
    off = 0
    _, off = read_u32(priv_blob, off)
    _, off = read_u32(priv_blob, off)
    key_type, off = read_string(priv_blob, off)
    print(f"Key type: {key_type.decode('utf-8')}")
    pubkey_blob, off = read_string(priv_blob, off)
    relying_party, off = read_string(priv_blob, off)
    print(f"Relying party: {relying_party.decode('utf-8')}")
    flags, off = read_byte(priv_blob, off)
    key_handle, off = read_string(priv_blob, off)
    return (pubkey_blob, key_handle)

def load_openssh_private(data_bytes):
    b = data_bytes.strip()
    if b.startswith(b"-----BEGIN OPENSSH PRIVATE KEY-----"):
        body = b.split(b"-----BEGIN OPENSSH PRIVATE KEY-----",1)[1]
        body = body.split(b"-----END OPENSSH PRIVATE KEY-----",1)[0]
        body = b"".join(body.splitlines())
        raw = base64.b64decode(body)
    else:
        # try base64 decode; if invalid, assume already binary openssh blob
        try:
            raw = base64.b64decode(b, validate=True)
        except Exception:
            raw = data_bytes
    return raw

def extract_from_file(path):
    data = sys.stdin.buffer.read() if path == "-" else open(path, "rb").read()
    raw = load_openssh_private(data)
    magic = b"openssh-key-v1\0"
    if not raw.startswith(magic):
        raise ValueError("not an openssh-key-v1 private key (magic mismatch)")
    off = len(magic)
    # string ciphername, string kdfname, string kdfoptions, uint32 nkeys
    _, off = read_string(raw, off)
    _, off = read_string(raw, off)
    _, off = read_string(raw, off)
    nkeys, off = read_u32(raw, off)
    for _ in range(nkeys):
        _, off = read_string(raw, off)   # public key blob(s)
    priv_block, off = read_string(raw, off)
    return extract_key_info(priv_block)

def main():
    p = argparse.ArgumentParser(description="Extract FIDO/U2F key_handle (credential id) from an OpenSSH sk private key")
    p.add_argument("infile", nargs="?", default="./testing_id_ed25519_sk", help="private key file (PEM or base64). Use '-' for stdin")
    args, _ = p.parse_known_args()
    infile = Path(args.infile)

    try:
        (pubkey_blob, key_handle) = extract_from_file(infile)
    except Exception as e:
        sys.stderr.write("ERROR: " + str(e) + "\n")
        sys.exit(2)

    name = infile.stem.upper()
    pubkey_formatted = ', '.join(f"0x{byte:02x}" for byte in pubkey_blob)
    print(f"pub const {name}_PUB: [u8; 32] = [{pubkey_formatted}];")
    print(f"pubkey base64: {base64.b64encode(pubkey_blob)}")
    key_handle_formatted = ', '.join(f"0x{byte:02x}" for byte in key_handle)
    print(f"pub const {name}_CRED_ID: [u8; {len(key_handle)}] = [{key_handle_formatted}];")
    print(f"cred id base64: {base64.b64encode(key_handle)}")

if __name__ == "__main__":
    main()
