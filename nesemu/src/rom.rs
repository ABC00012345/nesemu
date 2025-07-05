use std::{fs::File, io::{Error, ErrorKind, Read, Result}};

pub struct Rom {
    pub prg_rom : Vec<u8>,
    pub chr_rom: Vec<u8>,
}

impl Rom {
    pub fn check_magic(magic_bytes: &[u8]) -> bool {
        return magic_bytes == b"NES\x1A"
    }

    pub fn parse(mut rom_file: File) -> Result<Rom> {
        let mut rom = Vec::new();
        rom_file.read_to_end(&mut rom)?;

        println!("Read {} bytes from ROM", rom.len());

        // Check minimum length (16-byte header)
        if rom.len() < 16 {
            return Err(Error::new(ErrorKind::InvalidData, "ROM too short to contain NES header"));
        }

        // Check magic bytes
        if !Self::check_magic(&rom[0..4]) {
            return Err(Error::new(ErrorKind::InvalidData, "Invalid magic bytes: not a NES ROM"));
        }

        // Parse header
        let prg_rom_size = rom[4] as usize * 16 * 1024; // PRG-ROM size in bytes (16KB units)
        let chr_rom_size = rom[5] as usize * 8 * 1024;   // CHR-ROM size in bytes (8KB units)

        let flags6 = rom[6];
        let flags7 = rom[7];
        
        let has_trainer = (flags6 & 0b00000100) != 0; // Trainer present?
        let mapper_low = flags6 >> 4;
        let mapper_high = flags7 >> 4;
        let mapper = (mapper_high << 4) | mapper_low;

        println!("PRG-ROM size: {} KB", prg_rom_size / 1024);
        println!("CHR-ROM size: {} KB", chr_rom_size / 1024);
        println!("Mapper: {}", mapper);
        println!("Has trainer: {}", has_trainer);

        // Calculate where PRG-ROM and CHR-ROM start
        let mut offset = 16; // Skip header

        // Skip trainer (if present)
        if has_trainer {
            offset += 512; // Trainer is always 512 bytes
        }

        // Extract PRG-ROM (CPU instructions)
        let prg_rom = rom[offset..offset + prg_rom_size].to_vec();
        offset += prg_rom_size;

        // Extract CHR-ROM (Graphics data)
        let chr_rom = rom[offset..offset + chr_rom_size].to_vec();
        offset += chr_rom_size;

        println!("PRG-ROM starts at 0x{:X}, ends at 0x{:X}", 16, 16 + prg_rom_size);
        println!("CHR-ROM starts at 0x{:X}, ends at 0x{:X}", 16 + prg_rom_size, 16 + prg_rom_size + chr_rom_size);

        println!("\nFirst few PRG-ROM bytes (opcodes):");
        for &byte in prg_rom.iter().take(16) {
            print!("{:02X} ", byte);
        }
        println!();

        let reset_vector = {
            let lo = prg_rom[prg_rom.len() - 4] as u16;
            let hi = prg_rom[prg_rom.len() - 3] as u16;
            (hi << 8) | lo
        };

        println!("Reset vector: ${:04X}", reset_vector);

        let prg_rom_start = 16;
        let chr_rom_start = prg_rom_start + prg_rom_size;
        
        Ok(Rom {
            prg_rom: rom[prg_rom_start..chr_rom_start].to_vec(),
            chr_rom: rom[chr_rom_start..chr_rom_start + chr_rom_size].to_vec(),
        })
    }
}
