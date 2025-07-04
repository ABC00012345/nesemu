use std::fs::File;
use std::io::Result;

mod cpu;
mod rom;

fn main() -> Result<()> {
    // test for now
    let rom_file = File::open("/media/lukas/MYFILES/backups_all_win/Documents_win/hackclub_ALL_PROJECTS/emulators/nes_emulator/nesemu/src/cpu_dummy_reads.nes")?;

    rom::parse(rom_file)?;

    Ok(())

}
