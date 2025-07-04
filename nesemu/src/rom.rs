use std::{fs::File, io::{Error, ErrorKind, Read, Result}};


pub fn check_magic(magic_bytes: &[u8]) -> bool {
    return magic_bytes == b"NES"
}

pub fn parse(mut rom_file: File) -> Result<()> {

    let mut rom = Vec::new();              // dynamic byte buffer
    rom_file.read_to_end(&mut rom)?;           // fill it with file contents

    println!("Read {} bytes from ROM", rom.len());

    println!("Running ROM file checks ...");

    if rom.len() < 3 {
        return Err(Error::new(ErrorKind::InvalidData, "ROM too short to contain NES header"));
    }

    if !check_magic(&rom[0..3]) {
        return Err(Error::new(ErrorKind::InvalidData, "Invalid magic bytes: not a NES ROM"));
    }


    Ok(())
}