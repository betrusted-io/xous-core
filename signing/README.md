# Image Signing and Deployment

The scripts in this directory are used for image signing and deployment.

The primary method for securing images is by signing them with a Precursor or Baochip device. There is a `beta` key, which is kept within such a device, but the device is regularly accessible for testing purposes. There is also at least one `deployment` key, which is kept in a similar device, but the device is kept in a physically secured location and deploying it requires some ceremony and effort.

Here is the flow for generating and deploying signatures:

## Preparation
1. The "HSM" (as it may be -- really just a signing token) is a Precursor/Baochip device. The signature type is `ed25519`. The device is primed with a PIN like a normal FIDO device.
1. A residential SSH key is created on the device using `ssh-keygen -t ed25519-sk`
1. The "private key" resulting from this contains the credential ID, public key, relying party,
2. Extract the credentials using e.g. `python3 .\extract_sk_credential.py .\bao2_id_ed25519_sk`
3. Paste the `_PUB` and `_CRED_ID` into the respective public key slot in `bao1x-api/src/pubkeys/`
4. Activate the `tag` field in the `PUBKEY` record. This will cause it to be considered for checking signatures.
5. Copy the base-64 representation of the cred-id into e.g. `credentials/bao2.json` so that `fido-signer` can refer to it. Note that the token also needs to have a PIN setup.

## Signing
2. The image to be signed is fed into `fido-signer`:
   1. The signing token must be connected to the computer
   2. The PIN must be provided by the signer
   3. Depending on the host, this may need to run at an elevated privilege level (Windows in particular enforces that)
3. Assuming the prequisites are met, `fido-signer` patches a unified signed Baochip image to inject the `signature` and `auth_data` into the designated key slot. It also generates a `.uf2` file for convenience.

Example command line of `fido-signer`, as run from the `signing/fido-signer` directory:

`cargo run -- --credential-file ..\credentials\bao2.json --file ..\..\target\riscv32imac-unknown-none-elf\release\bao1x-boot0.img --function-code boot0`

The `--function-code` argument is necessary to create the .uf2 file. Without it, only a .img file is updated.

In the case that a `deployment` key is used, additional physical security measures are invoked, a list of which are not publicly documented because in physical security, obscurity and the element of surprise are valid and useful countermeasures.

## Other notes

The `credentials` directory is in the `.gitignore` and should not be checked in. If you see a `credentials` directory in the tree, there was an operational security problem.

That being said, `credentials` does *not* store the actual private key used for signing. The main thing the directory contains is the PIN to unlock the signing token, which is a secondary security precaution.

