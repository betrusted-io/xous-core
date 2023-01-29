// Copyright 2019-2021 Google LLC
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

pub const ATTESTATION_PRIVATE_KEY_LENGTH: usize = 32;
pub const AAGUID_LENGTH: usize = 16;

// NOTE: we don't support AAGUID -- this would require authenticator
// certification, and, we're not going to do that. This just returns
// a bogus value.
pub const AAGUID: &[u8; AAGUID_LENGTH] =
    &[
        0x48, 0xe0, 0x37, 0x1c, 0x32, 0x52, 0x57, 0xf7,  0xd2, 0xf0, 0x00, 0x0f, 0xbd, 0x7a, 0x89, 0x19
    ];
