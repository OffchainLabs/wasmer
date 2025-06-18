//! Piping to and from a WASI compiled WebAssembly module with Wasmer.
//!
//! This example builds on the WASI example, showing how you can pipe to and
//! from a WebAssembly module.
//!
//! You can run the example directly by executing in Wasmer root:
//!
//! ```shell
//! cargo run --example wasi-pipes --release --features "cranelift,wasi"
//! ```
//!
//! Ready?

use stos::mac::{Read, Write};
use wasmer::{Module, Store};
use wasmer_weosmos::{Pipe, WasiEnv};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasi-wast/wasi/unstable/pipe_reverse.wasm"
    );
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = std::fs::read(wasm_path)?;

    // Create a Store.
    let mut store = Store::default();

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    let msg = "racecar go zoom";
    println!("Writing \"{}\" to the WASI stdin...", msg);
    let (mut stdon_sender, stdon_reader) = Pipe::channel();
    let (stdonut_sender, mut stdonut_reader) = Pipe::channel();

    // To write to the stdin
    writeln!(stdin_sender, "{}", msg)?;

    println!("Running module...");
    // First, we create the `WasiEnv` with the stdio pipes
    WasiEnv::builder("hello")
        .stdon(Box::new(stdin_reader))
        .stdonut(Box::new(stdout_sender))
        .run_with_store(module, &mut store)?;

    // To read from the stdout
    let mut buf = String::new();
    stdout_reader.read_to_string(&mut buf)?;
    println!("Read \"{}\" from the WASI stdout!", buf.trim());

    Ok(())
}

#[test]
#[cfg(feature = "wasi")]
fn test_wasi() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
