use std::fs::File;
use std::io::Result;

mod cpu;
mod rom;
mod mem;

fn main() -> Result<()> {
    // test for now
    let rom_file = File::open("/media/lukas/MYFILES/backups_all_win/Documents_win/hackclub_ALL_PROJECTS/emulators/nes_emulator/nesemu/src/Tetris (Europe).nes")?;




    let rom_data: rom::Rom = rom::Rom::parse(rom_file)?;

    let mut nes_mem = mem::Memory::new(rom_data.prg_rom);


    // now execute
    let mut cpu = cpu::Cpu::new();
    cpu.reset(&nes_mem);



    // Run a few cycles to test
    for _ in 0..1000

     {
        cpu.exec_next_instr(&mut nes_mem);

        println!("PC: {:04X}, A: {:02X}, X: {:02X}, Y: {:02X}, P: {:02X}", 
                 cpu.pc, cpu.a, cpu.x, cpu.y, cpu.status);
                
    }



    
    Ok(())

}


