fn main() {
    if let Err(error) = napomni::local_host::run_cli(std::env::args().skip(1).collect()) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
