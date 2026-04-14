use std::env;

fn main() {
    let root = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    if let Err(err) = colmem_core::mcp::serve_stdio(&root) {
        eprintln!("colmem-mcp failed: {err}");
        std::process::exit(1);
    }
}
