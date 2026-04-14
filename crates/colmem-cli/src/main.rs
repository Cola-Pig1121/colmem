use std::env;
fn main() {
    let cwd = env::current_dir().unwrap_or_else(|_| ".".into());
    match colmem_cli::run_in_dir(env::args().skip(1).collect(), &cwd) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}
