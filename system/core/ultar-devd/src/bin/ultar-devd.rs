//! The device daemon of Ultar OS (plan §5.4). See `ultar_devd::runner`.

#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = ultar_devd::runner::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
