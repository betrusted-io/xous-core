fn main() {
    println!("Loading stub file");
    let spawn_stub = include_bytes!("spawn-stub");
    println!("About to launch a process...");

    let args = xous::ProcessArgs::new(spawn_stub, xous::MemoryAddress::new(0x2050_1000).unwrap());
    let process = xous::create_process(args).unwrap();
    println!("Connected to process. PID: {:?}, CID: {:?}", process.pid, process.cid);
    let result = xous::send_message(process.cid, xous::Message::new_blocking_scalar(4, 1, 2, 3, 4)).unwrap();
    println!("Result of ping: {:?}", result);
    println!("Hello, world!");
}
