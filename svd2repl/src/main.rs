mod generate;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().count() != 3 {
        println!("Usage: svd2repl <input SVD> <output repl>");
        return Ok(())
    }
    let svd_filename = std::env::args().nth(1).ok_or("Must specify SVD input filename")?;
    let generated_filename = std::env::args().nth(2).ok_or("Must specify destination repl filename")?;

    let src_file = std::fs::File::open(svd_filename).expect("couldn't open src file");
    let mut dest_file = std::fs::File::create(generated_filename).expect("couldn't open dest file");

    generate::generate(src_file, &mut dest_file)?;

    Ok(())
}
