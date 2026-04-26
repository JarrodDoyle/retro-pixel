use std::env;

use anyhow::Result;

mod cli;
mod gui;
mod image;

fn main() -> Result<()> {
    if env::args().count() > 1 {
        cli::run()?;
    } else {
        gui::run()?;
    }

    Ok(())
}
