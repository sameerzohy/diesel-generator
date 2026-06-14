mod cli;
mod ir;
mod parser;
mod typemap;
mod config;
mod codegen;
mod verify;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
