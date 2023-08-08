use std::env;
use crate::builder::CrateSpec;
use std::path::Path;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read};

use crate::DynError;

pub fn check_project_consistency() -> Result<(), DynError> {
    // note: implementations no longer published as crates: just APIs as of March 2023
    // TODO: retire utralib/svd2utra from publication as well
    let check_pkgs = [
        // this set updates with kernel API changes
        "xous@0.9.50",
        "xous-ipc@0.9.50",
        "xous-api-log@0.1.46",
        "xous-api-names@0.9.48",
        "xous-api-susres@0.9.46",
        "xous-api-ticktimer@0.9.46",
    ];
    // utra/svd2utra changes are downgraded to warnings because these now prefer to pull
    // from the local patch version, so any inconsistency simply indicates we forgot to
    // publish the packages, rather than something nefarious has happened.
    let warn_pkgs = [
        // this set is only updated if the utralib changes
        "utralib@0.1.22",
        "svd2utra@0.1.20",
    ];
    for pkg in check_pkgs {
        verify(pkg.into(), true)?;
    }
    for pkg in warn_pkgs {
        verify(pkg.into(), false)?;
    }
    Ok(())
}

pub fn verify(spec: CrateSpec, hard_failure: bool) -> Result<(), DynError> {
    if let CrateSpec::CratesIo(name, version, _xip) = spec {
        let mut cache_path = Path::new(&env::var("CARGO_HOME").unwrap()).to_path_buf();
        cache_path.push("registry");
        cache_path.push("src");
        let mut cache_leaf = String::new();
        for entry in fs::read_dir(&cache_path)? {
            let entry = entry?;
            let path = entry.path();
            // this should *really* exist if the build system is stable, so just unwrap all the things
            let regdir = path.file_name().unwrap().to_str().unwrap().to_string();
            if regdir.contains("crates.io") { // crates.io sticks sources in something with git yadda yadda...docs don't really say what/why/how...
                cache_leaf.push_str(&regdir);
            }
        }
        if cache_leaf.len() == 0 {
            return Err("Can't find expected registry source location".into())
        }
        // this now has the path to the cache directory
        cache_path.push(cache_leaf);
        // form the package source name
        cache_path.push(format!("{}-{}", name, version));
        if !cache_path.exists() {
            println!("Package not in registry, skipping consistency check: {}", cache_path.as_os_str().to_str().unwrap());
            return Ok(());
        }

        // form the local source path
        let subdir = if name.contains("-api-") {
            "api"
        } else {
            "services"
        };
        let subdir = format!("./{}/{}/", subdir, name);
        // handle special cases of xous kernel and ipc crates
        let src_path = if name != "xous" && name != "xous-ipc" && name != "xous-kernel"
        && name != "utralib" && name != "svd2utra" {
            Path::new(&subdir)
        } else {
            if name == "xous" {
                Path::new("./xous-rs")
            } else if name == "xous-ipc" {
                Path::new("./xous-ipc")
            } else if name == "xous-kernel" {
                Path::new("./kernel")
            } else if name == "utralib" {
                Path::new("./utralib")
            } else if name == "svd2utra" {
                Path::new("./svd2utra")
            } else {
                panic!("Consistency error: special case handling did not find either xous or xous-ipc");
            }
        };

        // now recurse through the source path and check that it matches the cache, except for Cargo.toml
        println!("Comparing {} <-> {}", src_path.as_os_str().to_str().unwrap(), cache_path.as_os_str().to_str().unwrap());
        match compare_dirs(src_path, &cache_path) {
            Ok(true) => Ok(()),
            Ok(false) => if hard_failure {
                    Err("Crates.io downloaded data does not match local source".into())}
                else {
                    println!("**WARNING**: Local package does not match the published source. Third parties downloading from crates.io will be inconsistent with this build.");
                    Ok(())
                },
            _ => Err("Error matching local source to crates.io cache files".into()),
        }
    } else {
        Err("Can't verify crates that aren't from crates.io".into())
    }

}

fn compare_dirs(src: &Path, other: &Path) -> Result<bool, DynError> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let fname = entry.file_name();
            if fname.as_os_str().to_str().unwrap() == "Cargo.toml" {
                /*
                    This is awful. The Cargo.toml file is parsed and reformatted by the packaging tool to normalize its contents.
                    Thus, the Cargo.toml file of the downloaded version never matches the Cargo.toml file that's actually used.
                    Unfortunately, there doesn't seem to be an easy way to check the equivalence of two Cargo.toml files,
                    except for recursively and deeply parsing through and comparing all the possibile keys and values of
                    the abstract key/value tree.

                    As a hack, we compare to the Cargo.toml.orig file. Which is...kind of OK, but really, this opens us
                    up to attacks where someone just has to replace a version on a package or even just swap out an
                    entire package for a malicious one by just using package name re-assignment which is a thing that
                    the format supports. In other words, all this checking is kind of pointless because it's super-easy
                    to swap out key crates for whole other crates and have it go undetected.
                 */
                let mut other_file = other.to_path_buf();
                other_file.push("Cargo.toml.orig");
                let mut src_file = src.to_path_buf();
                src_file.push(&fname);
                // println!("comparing {} <-> {}", src_file.as_os_str().to_str().unwrap(), other_file.as_os_str().to_str().unwrap());
                match compare_files(&src_file, &other_file) {
                    Ok(true) => {},
                    Ok(false) => {
                        println!("Cargo.toml FAIL: {} <-> {}", src_file.as_os_str().to_str().unwrap(), other_file.as_os_str().to_str().unwrap());
                        return Ok(false)
                    },
                    Err(_) => return Err("Access error comparing remote and local crates".into())
                }
                // Cargo.toml's do *not* match
                /* turns out it's *really hard* to check equivalence of cargo files...you have to deep parse it into all the values.
                let toml_src_file = fs::read_to_string(entry.path())?;
                let toml_src = toml_src_file.parse::<Document>().expect("invalid source toml");
                let mut other_file = other.to_path_buf();
                other_file.push(&fname);
                let toml_other_file = fs::read_to_string(&other_file)?;
                let toml_other = toml_other_file.parse::<Document>().expect("invalid remote toml");
                println!("values: {}", toml_src.iter().count());
                if toml_src.iter().count() != toml_other.iter().count() {
                    println!("CARGO LEN FAIL: {} <-> {}", toml_src.get_values().len(), toml_other.get_values().len());
                    return Ok(false)
                }
                for ((astr, aitem), (bstr, bitem)) in toml_src.iter().zip(toml_other.iter()) {
                    println!("{}, {}", astr, bstr);
                    if astr != bstr {
                        println!("CARGO KEY FAIL: {:?} <-> {:?}", astr, bstr);
                        return Ok(false)
                    }
                    // this is a failed attempt to just print the "item" data within a block; but,
                    // this data is not parsed into some abstract format, and you'll get all the comments and stuff
                    // which doesn't match between the files
                    use std::fmt::Debug;
                    let adbg = format!("{:?}", aitem);
                    let bdbg = format!("{:?}", bitem);
                    println!("{:?}, {:?}", adbg, bdbg);
                    if adbg != bdbg {
                        println!("CARGO ITEM FAIL: {:?} <-> {:?}", adbg, bdbg);
                        return Ok(false)
                    }
                }
                // below was a failed attempt to iterate through all the key/value pairs but this doesn't work
                // because get_values() doesn't actually give you all the values contained within the Toml file,
                // It returns 0 values for a top level Toml file; I think you have to recursively descend into
                // the abstract representation to make this work.
                for ((av, a), (bv, b)) in toml_src.get_values().iter().zip(toml_other.get_values().iter()) {
                    println!("value: {:?}", a.as_str());
                    if a.as_str() != b.as_str() {
                        println!("CARGO VALUE FAIL: {:?} <-> {:?}", a.as_str(), b.as_str());
                        return Ok(false)
                    }
                    println!("kvlen: {}", av.len());
                    if av.len() != bv.len() {
                        println!("CARGO KEYCOUNT FAIL: {} <-> {}", av.len(), bv.len());
                        return Ok(false)
                    }
                    for (&akey, &bkey) in av.iter().zip(bv.iter()) {
                        println!("key: {}", akey.get());
                        if akey.get() != bkey.get() {
                            println!("CARGO KEY FAIL: {} <-> {}", akey.get(), bkey.get());
                            return Ok(false)
                        }
                    }
                }
                */
                // things matched, go to the next file
                continue;
            } else {
                let mut other_file = other.to_path_buf();
                other_file.push(&fname);
                let mut src_file = src.to_path_buf();
                src_file.push(&fname);
                // println!("comparing {} <-> {}", src_file.as_os_str().to_str().unwrap(), other_file.as_os_str().to_str().unwrap());
                if (src_file.as_os_str().to_str().unwrap().contains("ticktimer")
                    && fname.as_os_str().to_str().unwrap() == "version.rs")
                    || src_file.as_os_str().to_str().unwrap().contains("Cargo.lock") {
                    // don't compare the version.rs, as it's supposed to be different due to the timestamp
                    // don't compare Cargo.lock files that happen to be checked into packages
                    // println!("skipping ticktimer version.rs or Cargo.lock file"); // this line is helpful to ensure our skip exception isn't too broad.
                } else {
                    match compare_files(&src_file, &other_file) {
                        Ok(true) => {},
                        Ok(false) => {
                            println!("DIFF FAIL: {} <-> {}", src_file.as_os_str().to_str().unwrap(), other_file.as_os_str().to_str().unwrap());
                            return Ok(false)
                        },
                        Err(_) => return Err("Access error comparing remote and local crates".into())
                    }
                }
            }
        } else if entry.file_type()?.is_dir() {
            let dname = entry.file_name();
            if dname.as_os_str().to_str().unwrap() == "target" {
                // don't match on target directory
                continue;
            }
            let mut other_dir = other.to_path_buf();
            other_dir.push(&dname);
            let mut src_dir = src.to_path_buf();
            src_dir.push(&dname);
            // println!("comparing {}/ <-> {}/", src_dir.as_os_str().to_str().unwrap(), &other_dir.as_os_str().to_str().unwrap());
            match compare_dirs(&src_dir, &other_dir) {
                Ok(true) => {},
                Ok(false) => {
                    println!("DIR FAIL: {}/ <-> {}/", src_dir.as_os_str().to_str().unwrap(), &other_dir.as_os_str().to_str().unwrap());
                    return Ok(false)
                },
                Err(_) => return Err("Access error comparing remote to local crates".into())
            };
        }
    }
    Ok(true)
}

fn compare_files(a: &Path, b: &Path) -> Result<bool, DynError> {
    let f1 = File::open(a)?;
    let f2 = File::open(b)?;

    // check if file sizes are the same
    // HAHA joke is on me, this doesn't work because CRLF->LF conversions.
    // if f1.metadata().unwrap().len() != f2.metadata().unwrap().len() {
    //     return Ok(false);
    // }

    let s1 = fs::read_to_string(a);
    let s2 = fs::read_to_string(b);

    if s1.is_ok() && s2.is_ok() {
        // text do CRLF substitutions
        let f1 = s1?.replace("\r\n", "\n");
        let f2 = s2?.replace("\r\n", "\n");
        if f1 == f2 {
            Ok(true)
        } else {
            Ok(false)
        }
    } else {
        // do a binary compare
        let f1 = BufReader::new(f1);
        let f2 = BufReader::new(f2);

        // Do a byte to byte comparison of the two files
        for (b1, b2) in f1.bytes().zip(f2.bytes()) {
            if b1.unwrap() != b2.unwrap() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}