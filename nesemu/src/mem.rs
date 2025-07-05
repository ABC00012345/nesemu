use std::ops::RangeInclusive;

pub struct Memory {
    cpu_ram: [u8; 0x0800],       // $0000-$07FF
    prg_rom: Vec<u8>,           // $8000-$FFFF (external)
    cartridge_ram: [u8; 0x2000],// $6000-$7FFF (optional save RAM)
    ppu_registers: [u8; 8],     // $2000-$2007
    apu_io_registers: [u8; 0x18], // $4000-$4017
    oam_dma: u8,                // $4014 (DMA trigger)
}

impl Memory {
    pub fn new(prg_rom: Vec<u8>) -> Self {
        Self {
            cpu_ram: [0; 0x0800],
            prg_rom,
            cartridge_ram: [0; 0x2000],
            ppu_registers: [0; 8],
            apu_io_registers: [0; 0x18],
            oam_dma: 0,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            // CPU internal RAM (mirrored every 0x800 bytes)
            0x0000..=0x1FFF => {
                let mirror_addr = addr as usize % 0x0800;
                self.cpu_ram[mirror_addr]
            }
            // PPU registers (mirrored every 8 bytes)
            0x2000..=0x3FFF => {
                let reg = (addr - 0x2000) % 8;
                self.ppu_registers[reg as usize]
            }
            // APU and I/O
            0x4000..=0x4013 | 0x4015 => {
                self.apu_io_registers[(addr - 0x4000) as usize]
            }
            0x4014 => self.oam_dma,
            // Cartridge RAM (optional save RAM)
            0x6000..=0x7FFF => {
                self.cartridge_ram[(addr - 0x6000) as usize]
            }
            // PRG-ROM (no mirroring)
            0x8000..=0xFFFF => {
                let prg_addr = (addr - 0x8000) as usize % self.prg_rom.len();
                self.prg_rom[prg_addr]
            }
            _ => 0 // Unmapped areas return 0
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // CPU internal RAM
            0x0000..=0x1FFF => {
                let mirror_addr = addr as usize % 0x0800;
                self.cpu_ram[mirror_addr] = value;
            }
            // PPU registers
            0x2000..=0x3FFF => {
                let reg = (addr - 0x2000) % 8;
                self.ppu_registers[reg as usize] = value;
            }
            // APU and I/O
            0x4000..=0x4013 | 0x4015 => {
                self.apu_io_registers[(addr - 0x4000) as usize] = value;
            }
            0x4014 => {
                self.oam_dma = value;
                // NOTE: OAM DMA logic would be handled by CPU
            }
            // Cartridge SRAM
            0x6000..=0x7FFF => {
                self.cartridge_ram[(addr - 0x6000) as usize] = value;
            }
            // PRG-ROM is read-only
            0x8000..=0xFFFF => {
                // Ignore writes to ROM
            }
            _ => {}
        }
    }

    pub fn load_prg_rom(&mut self, new_prg: Vec<u8>) {
        self.prg_rom = new_prg;
    }

    pub fn reset(&mut self) {
        self.cpu_ram = [0; 0x0800];
        self.cartridge_ram = [0; 0x2000];
        self.ppu_registers = [0; 8];
        self.apu_io_registers = [0; 0x18];
        self.oam_dma = 0;
    }

    pub fn read_u16(&self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
}
