//! Owner and mode of the seat's device nodes (part of ultar-devd). See
//! `ultar_devd::seat`.

#![forbid(unsafe_code)]

fn main() {
    // `run` may start the service runtime, which must happen before any other
    // thread exists: nothing else may come first here.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = ultar_devd::seat::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
