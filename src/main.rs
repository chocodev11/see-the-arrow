mod cli;
mod model;
mod platform;
mod render;

fn main() {
    platform::init();
    std::process::exit(cli::main());
}
