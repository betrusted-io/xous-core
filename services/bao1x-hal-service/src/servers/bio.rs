pub fn start_bio_service() {
    std::thread::spawn(move || {
        bio_service();
    });
}

fn bio_service() {}
