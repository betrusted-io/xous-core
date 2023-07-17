// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-FileCopyrightText: 2020 bunnie <bunnie@kosagi.com>
// SPDX-License-Identifier: MIT OR Apache-2.0

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().count() != 3 {
        println!("Usage: svd2utra <input SVD> <output utra>");
        return Ok(())
    }
    let svd_filename = std::env::args().nth(1).ok_or("Must specify SVD input filename")?;
    let generated_filename = std::env::args().nth(2).ok_or("Must specify destination utralib filename")?;

    let mut dest_file = std::fs::File::create(generated_filename).expect("couldn't open dest file");
    let src_file = std::fs::File::open(svd_filename).expect("couldn't open source file");

    svd2utra::generate(vec![src_file], &mut dest_file)?;

    Ok(())
}
