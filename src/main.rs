mod cli;
mod ir;
mod parser;
mod typemap;
mod codegen;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
