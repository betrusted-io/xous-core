// Copyright 2019-2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::stringify;

/// Number of keys that persist the CTAP reset command.
#[allow(dead_code)] // OpenSK legacy
pub const NUM_PERSISTENT_KEYS: usize = 20;

/// Helper to define keys as a partial partition of a range.
macro_rules! define_names {
     ($(
         $(#[$doc: meta])*
         $name: ident = $key: literal $(.. $end: literal)?;
     )*) => {
        $(
            pub const $name: &'static str = stringify!($name);
        )*
    };
}

define_names! {
    // WARNING: Keys should not be deleted but prefixed with `_` to avoid accidentally reusing them.
    // the range information is not used by Xous' PDDB, but it is kept around for easy comparison against
    // the original OpenSK reference code.

    /// The attestation private key.
    ATTESTATION_PRIVATE_KEY = 1;

    /// The attestation certificate.
    ATTESTATION_CERTIFICATE = 2;

    /// The aaguid.
    AAGUID = 3;

    // In Xous, you can have as many persistent keys as you want, but, you just have to put them
    // in the FIDO_PERSISTENT_DICT. This is controlled in the implementation in the storage.rs file.

    // This is the persistent key limit:
    // - When adding a (persistent) key above this message, make sure its value is smaller than
    //   NUM_PERSISTENT_KEYS.
    // - When adding a (non-persistent) key below this message, make sure its value is bigger or
    //   equal than NUM_PERSISTENT_KEYS.

    /// Reserved for future credential-related objects.
    ///
    /// In particular, additional credentials could be added there by reducing the lower bound of
    /// the credential range below as well as the upper bound of this range in a similar manner.
    _RESERVED_CREDENTIALS = 1000..1700;

    /// The credentials.
    ///
    /// Depending on `MAX_SUPPORTED_RESIDENTIAL_KEYS`, only a prefix of those keys is used. Each
    /// board may configure `MAX_SUPPORTED_RESIDENTIAL_KEYS` depending on the storage size.
    // CREDENTIALS = 1700..2000;

    /// The secret of the CredRandom feature.
    CRED_RANDOM_SECRET = 2041;

    /// List of RP IDs allowed to read the minimum PIN length.
    #[cfg(feature = "with_ctap2_1")]
    _MIN_PIN_LENGTH_RP_IDS = 2042;

    /// The minimum PIN length.
    ///
    /// If the entry is absent, the minimum PIN length is `DEFAULT_MIN_PIN_LENGTH`.
    #[cfg(feature = "with_ctap2_1")]
    MIN_PIN_LENGTH = 2043;

    /// The number of PIN retries.
    ///
    /// If the entry is absent, the number of PIN retries is `MAX_PIN_RETRIES`.
    PIN_RETRIES = 2044;

    /// The PIN hash.
    ///
    /// If the entry is absent, there is no PIN set.
    PIN_HASH = 2045;

    /// The encryption and hmac keys.
    ///
    /// This entry is always present. It is generated at startup if absent.
    MASTER_KEYS = 2046;

    /// The global signature counter.
    ///
    /// If the entry is absent, the counter is 0.
    GLOBAL_SIGNATURE_COUNTER = 2047;
}
