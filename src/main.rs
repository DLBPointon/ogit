use human_panic::setup_panic;
use ogit::run;

// https://doc.rust-lang.org/book/ch12-03-improving-error-handling-and-modularity.html#separation-of-concerns-for-binary-projects
fn main() {
    println!("hello kitty - From Winnie (6yo)");

    //  https://rust-cli.github.io/book/in-depth/human-communcation.html
    setup_panic!();
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    } else {
        println!("Done!");
    }
}
