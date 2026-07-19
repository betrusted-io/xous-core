use criterion::{Criterion, criterion_group, criterion_main};
use signature::{Keypair, Signer, Verifier};
use slh_dsa::*;
use std::hint::black_box;

pub fn sign_benchmark<P: ParameterSet>(c: &mut Criterion) {
    let mut rng = rand::rng();
    let sk = SigningKey::<P>::new(&mut rng);
    c.bench_function(&format!("sign: {}", P::NAME), |b| {
        b.iter(|| {
            let msg = b"Hello, world!";
            let sig = sk.try_sign(msg).unwrap();
            black_box(sig)
        })
    });
}

pub fn verify_benchmark<P: ParameterSet>(c: &mut Criterion) {
    let mut rng = rand::rng();
    let sk = SigningKey::<P>::new(&mut rng);
    let msg = b"Hello, world!";
    let sig = sk.try_sign(msg).unwrap();
    let vk = sk.verifying_key();
    c.bench_function(&format!("verify: {}", P::NAME), |b| {
        b.iter(|| {
            let ok = vk.verify(msg, &sig);
            black_box(ok)
        })
    });
}

criterion_group!(name = sign_benches;
    config = Criterion::default().sample_size(10);
    targets = sign_benchmark<Shake128s>, sign_benchmark<Shake192s>, sign_benchmark<Shake256s>,
              sign_benchmark<Shake128f>, sign_benchmark<Shake192f>, sign_benchmark<Shake256f>,
              sign_benchmark<Sha2_128s>, sign_benchmark<Sha2_192s>, sign_benchmark<Sha2_256s>,
              sign_benchmark<Sha2_128f>, sign_benchmark<Sha2_192f>, sign_benchmark<Sha2_256f>,
);

criterion_group!(name = verify_benches;
    config = Criterion::default().sample_size(10);
    targets = verify_benchmark<Shake128s>, verify_benchmark<Shake192s>, verify_benchmark<Shake256s>,
              verify_benchmark<Shake128f>, verify_benchmark<Shake192f>, verify_benchmark<Shake256f>,
              verify_benchmark<Sha2_128s>, verify_benchmark<Sha2_192s>, verify_benchmark<Sha2_256s>,
              verify_benchmark<Sha2_128f>, verify_benchmark<Sha2_192f>, verify_benchmark<Sha2_256f>,
);

criterion_main!(sign_benches, verify_benches);
