from cryptography import x509
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec

# openssl ecparam -genkey -name prime256v1 -out ec_key.key
# openssl req -new -key ec_key.key -out cert.csr -subj "/CN=Precursor CA"
# openssl x509 -trustout -req -days 7305 -in cert.csr -signkey ec_key.key -outform der -out cert.der -sha256
# pem2bytes.py > key.hex
with open("ec_key.key", "rb") as pem_f:
    pem = pem_f.read()
    priv_key = serialization.load_pem_private_key(pem, password=None)
    if not isinstance(priv_key, ec.EllipticCurvePrivateKey):
      print("Private key must be an Elliptic Curve one.")
    if not isinstance(priv_key.curve, ec.SECP256R1):
      print("Private key must use Secp256r1 curve.")
    if priv_key.key_size != 256:
      print("Private key must be 256 bits long.")
    print("Private key is valid.")
    print(priv_key.private_numbers().private_value.to_bytes(
                length=32, byteorder='big', signed=False).hex())