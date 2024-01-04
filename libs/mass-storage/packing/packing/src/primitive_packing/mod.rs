// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

#![allow(unused_imports)]

mod bool_pack;
pub use bool_pack::*;

mod u8_pack;
pub use u8_pack::*;

mod u16_pack;
pub use u16_pack::*;

mod u32_pack;
pub use u32_pack::*;

mod u64_pack;
pub use u64_pack::*;

mod u128_pack;
pub use u128_pack::*;

mod i8_pack;
pub use i8_pack::*;

mod i16_pack;
pub use i16_pack::*;

mod i32_pack;
pub use i32_pack::*;

mod i64_pack;
pub use i64_pack::*;

mod i128_pack;
pub use i128_pack::*;

mod f32_pack;
pub use f32_pack::*;

mod f64_pack;
pub use f64_pack::*;
