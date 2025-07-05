use std::{fs::{File, OpenOptions}, io::Write};

use crate::mem;

pub struct Cpu {
    pub pc: u16,     // Program Counter
    pub sp: u8,      // Stack Pointer
    pub a: u8,       // Accumulator
    pub x: u8,       // X Register
    pub y: u8,       // Y Register
    pub status: u8,  // Processor Status
}

// 6502 Status Flag Constants
const CARRY_FLAG: u8 = 0b0000_0001;     // Bit 0
const ZERO_FLAG: u8 = 0b0000_0010;      // Bit 1
const INTERRUPT_FLAG: u8 = 0b0000_0100; // Bit 2
const DECIMAL_FLAG: u8 = 0b0000_1000;   // Bit 3
const BREAK_FLAG: u8 = 0b0001_0000;     // Bit 4
const UNUSED_FLAG: u8 = 0b0010_0000;    // Bit 5
const OVERFLOW_FLAG: u8 = 0b0100_0000;  // Bit 6
const NEGATIVE_FLAG: u8 = 0b1000_0000;  // Bit 7


impl Cpu {
    pub fn new() -> Self {
        Self {
            pc: 0,
            sp: 0xFD,
            a: 0,
            x: 0,
            y: 0,
            status: 0x24, // unused & interrupt disable flags set
        }
    }

    pub fn reset(&mut self, memory: &mem::Memory) {
        self.pc = memory.read_u16(0xFFFC);
        println!("CPU PC: ${:04X}",self.pc);

    }

    fn update_zero_and_negative_flags(&mut self, result: u8) {
        self.status = (self.status & !(0b10 | 0b1000_0000))
            | if result == 0 { 0b10 } else { 0 }
            | if result & 0x80 != 0 { 0b1000_0000 } else { 0 };
    }

    fn push_u8(&mut self, memory: &mut mem::Memory, val: u8) {
        let addr = 0x0100 | self.sp as u16;
        memory.write(addr, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn push_u16(&mut self, memory: &mut mem::Memory, val: u16) {
        self.push_u8(memory, (val >> 8) as u8);
        self.push_u8(memory, (val & 0xFF) as u8);
    }

    fn pull_u16(&mut self, memory: &mut mem::Memory) -> u16 {
        self.sp = self.sp.wrapping_add(1);
        let lo = memory.read(0x0100 | self.sp as u16) as u16;
        self.sp = self.sp.wrapping_add(1);
        let hi = memory.read(0x0100 | self.sp as u16) as u16;
        (hi << 8) | lo
    }

    // Helper method to pull processor status from stack
    fn pull_status(&mut self, memory: &mut mem::Memory) {
        self.sp = self.sp.wrapping_add(1);
        let status = memory.read(0x0100 | self.sp as u16);
        // Note: Bits 4 and 5 are ignored when pulled (except for PHP)
        self.status = (status & 0b11001111) | (self.status & 0b00110000);
    }

    // ADC implementation
    fn adc(&mut self, memory: &mem::Memory, operand: u8) {
        let carry = (self.status & 0b0000_0001) as u16; // Get carry flag
        let a = self.a as u16;
        let m = operand as u16;
        let result = a + m + carry;

        // Update Carry flag (bit 0)
        self.status = if result > 0xFF { 
            self.status | 0b0000_0001 
        } else { 
            self.status & 0b1111_1110 
        };

        // Update Zero flag (bit 1)
        let result_u8 = result as u8;
        self.status = if result_u8 == 0 { 
            self.status | 0b0000_0010 
        } else { 
            self.status & 0b1111_1101 
        };

        // Update Negative flag (bit 7)
        self.status = if result_u8 & 0x80 != 0 { 
            self.status | 0b1000_0000 
        } else { 
            self.status & 0b0111_1111 
        };

        // Update Overflow flag (bit 6)
        // Overflow occurs when the sign of both inputs is the same,
        // and different from the result's sign
        let overflow = ((a ^ result) & (m ^ result) & 0x80) != 0;
        self.status = if overflow { 
            self.status | 0b0100_0000 
        } else { 
            self.status & 0b1011_1111 
        };

        self.a = result_u8;
    }

    // SBC implementation
    fn sbc(&mut self, memory: &mem::Memory, operand: u8) {
        // Invert the carry flag for subtraction (we borrow if carry is 0)
        let borrow = if (self.status & 0b0000_0001) == 0 { 1 } else { 0 };
        let a = self.a as u16;
        let m = operand as u16;
        let result = a.wrapping_sub(m).wrapping_sub(borrow);

        // Update Carry flag (bit 0) - set if result >= 0
        self.status = if result <= 0xFF {
            self.status | 0b0000_0001 
        } else { 
            self.status & 0b1111_1110 
        };

        // Update Zero flag (bit 1)
        let result_u8 = result as u8;
        self.status = if result_u8 == 0 { 
            self.status | 0b0000_0010 
        } else { 
            self.status & 0b1111_1101 
        };

        // Update Negative flag (bit 7)
        self.status = if result_u8 & 0x80 != 0 { 
            self.status | 0b1000_0000 
        } else { 
            self.status & 0b0111_1111 
        };

        // Update Overflow flag (bit 6)
        // Overflow occurs when the sign of the inputs differs and
        // the sign of the result differs from the accumulator
        let overflow = ((a ^ m) & (a ^ result) & 0x80) != 0;
        self.status = if overflow { 
            self.status | 0b0100_0000 
        } else { 
            self.status & 0b1011_1111 
        };

        self.a = result_u8;
    }

    // AND implementation
    fn and(&mut self, operand: u8) {
        self.a &= operand;
        self.update_zero_and_negative_flags(self.a);
    }

    // ORA implementation
    fn ora(&mut self, operand: u8) {
        self.a |= operand;
        self.update_zero_and_negative_flags(self.a);
    }

    // EOR implementation
    fn eor(&mut self, operand: u8) {
        self.a ^= operand;
        self.update_zero_and_negative_flags(self.a);
    }

    // BIT implementation
    fn bit(&mut self, memory: &mem::Memory, operand: u8) {
        // Set Zero flag based on A & operand
        self.status = if (self.a & operand) == 0 {
            self.status | 0b0000_0010  // Set Zero flag
        } else {
            self.status & 0b1111_1101  // Clear Zero flag
        };

        // Copy bit 7 of operand to Negative flag
        self.status = if (operand & 0x80) != 0 {
            self.status | 0b1000_0000  // Set Negative flag
        } else {
            self.status & 0b0111_1111  // Clear Negative flag
        };

        // Copy bit 6 of operand to Overflow flag
        self.status = if (operand & 0x40) != 0 {
            self.status | 0b0100_0000  // Set Overflow flag
        } else {
            self.status & 0b1011_1111  // Clear Overflow flag
        };
    }


    // ASL implementation
    fn asl(&mut self, memory: &mut mem::Memory, operand: u8, is_accumulator: bool) -> u8 {
        let result = operand << 1;
        
        // Update Carry flag (bit 0) with the shifted-out bit
        self.status = if (operand & 0x80) != 0 {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };
        
        self.update_zero_and_negative_flags(result);
        
        result
    }

    fn lsr(&mut self, memory: &mut mem::Memory, operand: u8, is_accumulator: bool) -> u8 {
        let result = operand >> 1;
        
        // Update Carry flag (bit 0) with the shifted-out bit
        self.status = if (operand & 0x01) != 0 {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };
        
        //self.update_zero_and_negative_flags(result);
        self.status &= 0b0111_1111;

        result
    }

    // ROL implementation
    fn rol(&mut self, memory: &mut mem::Memory, operand: u8, is_accumulator: bool) -> u8 {
        let carry_in = (self.status & 0b0000_0001) as u16;
        let result = ((operand as u16) << 1) | carry_in;
        
        // Update Carry flag (bit 0) with the shifted-out bit (bit 7)
        self.status = if (operand & 0x80) != 0 {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };
        
        let result_u8 = result as u8;
        self.update_zero_and_negative_flags(result_u8);
        
        result_u8
    }

    // ROR implementation
    fn ror(&mut self, memory: &mut mem::Memory, operand: u8, is_accumulator: bool) -> u8 {
        let carry_in = (self.status & 0b0000_0001) << 7; // Move carry to bit 7 position
        let result = (operand >> 1) | carry_in;
        
        // Update Carry flag (bit 0) with the shifted-out bit (bit 0)
        self.status = if (operand & 0x01) != 0 {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };
        
        self.update_zero_and_negative_flags(result);
        result
    }

    // CMP implementation
    fn cmp(&mut self, operand: u8) {
        let a = self.a as u16;
        let m = operand as u16;
        let result = a.wrapping_sub(m);

        // Update Carry flag (bit 0) - set if A >= operand
        self.status = if a >= m {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };

        // Update Zero flag (bit 1)
        self.status = if (result as u8) == 0 {
            self.status | 0b0000_0010
        } else {
            self.status & 0b1111_1101
        };

        // Update Negative flag (bit 7)
        self.status = if (result as u8) & 0x80 != 0 {
            self.status | 0b1000_0000
        } else {
            self.status & 0b0111_1111
        };
    }

    // CPX implementation
    fn cpx(&mut self, memory: &mem::Memory, operand: u8) {
        let x = self.x as u16;
        let m = operand as u16;
        let result = x.wrapping_sub(m);

        // Update Carry flag (bit 0) - set if X >= operand
        self.status = if x >= m {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };

        // Update Zero flag (bit 1)
        self.status = if (result as u8) == 0 {
            self.status | 0b0000_0010
        } else {
            self.status & 0b1111_1101
        };

        // Update Negative flag (bit 7)
        self.status = if (result as u8) & 0x80 != 0 {
            self.status | 0b1000_0000
        } else {
            self.status & 0b0111_1111
        };
    }

    // CPY implementation
    fn cpy(&mut self, memory: &mem::Memory, operand: u8) {
        let y = self.y as u16;
        let m = operand as u16;
        let result = y.wrapping_sub(m);

        // Update Carry flag (bit 0) - set if Y >= operand
        self.status = if y >= m {
            self.status | 0b0000_0001
        } else {
            self.status & 0b1111_1110
        };

        // Update Zero flag (bit 1)
        self.status = if (result as u8) == 0 {
            self.status | 0b0000_0010
        } else {
            self.status & 0b1111_1101
        };

        // Update Negative flag (bit 7)
        self.status = if (result as u8) & 0x80 != 0 {
            self.status | 0b1000_0000
        } else {
            self.status & 0b0111_1111
        };
    }


    pub fn exec_next_instr(&mut self, memory: &mut mem::Memory) {
        let opcode = memory.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        match opcode {
            // ----- LDA,LDX,LDY Instructions -----
            0xA9 => { // LDA Immediate
                let value = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.a = value;
                self.update_zero_and_negative_flags(self.a);
            }

            0xA5 => { // LDA Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
            }

            0xB5 => { // LDA Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
            }

            0xAD => { // LDA Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
            }

            0xBD => { // LDA Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
                // Optional: add cycle penalty if (base & 0xFF00) != (addr & 0xFF00)
            }

            0xB9 => { // LDA Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
                // Optional: add cycle penalty if page crossed
            }

            0xA1 => { // LDA (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
            }

            0xB1 => { // LDA (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
                // Optional: cycle penalty on page cross
            }

            0xA2 => { // LDX Immediate
                let value = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.x = value;
                self.update_zero_and_negative_flags(self.x);
            }

            0xA6 => { // LDX Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                self.x = memory.read(addr);
                self.update_zero_and_negative_flags(self.x);
            }

            0xB6 => { // LDX Zero Page,Y
                let addr = memory.read(self.pc).wrapping_add(self.y) as u16;
                self.pc = self.pc.wrapping_add(1);
                self.x = memory.read(addr);
                self.update_zero_and_negative_flags(self.x);
            }

            0xAE => { // LDX Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                self.x = memory.read(addr);
                self.update_zero_and_negative_flags(self.x);
            }

            0xBE => { // LDX Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                self.x = memory.read(addr);
                self.update_zero_and_negative_flags(self.x);
                // Optional: add cycle penalty if page crossed
            }

            0xA0 => { // LDY Immediate
                let value = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.y = value;
                self.update_zero_and_negative_flags(self.y);
            }

            0xA4 => { // LDY Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                self.y = memory.read(addr);
                self.update_zero_and_negative_flags(self.y);
            }

            0xB4 => { // LDY Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                self.y = memory.read(addr);
                self.update_zero_and_negative_flags(self.y);
            }

            0xAC => { // LDY Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                self.y = memory.read(addr);
                self.update_zero_and_negative_flags(self.y);
            }

            0xBC => { // LDY Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                self.y = memory.read(addr);
                self.update_zero_and_negative_flags(self.y);
                // Optional: add cycle penalty if page crossed
            }




            0x00 => { // BRK (Software interrupt)
                self.pc = self.pc.wrapping_add(1);
                self.push_u16(memory, self.pc);
                self.push_u8(memory, self.status | 0x10); // Set Break flag
                self.status |= 0x04; // Set Interrupt Disable
                self.pc = memory.read_u16(0xFFFE);
            }

            // ----- STA, STX, STY Instructions -----
            // STA instructions
            0x85 => { // STA Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                memory.write(addr, self.a);
            }

            0x95 => { // STA Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                memory.write(addr, self.a);
            }

            0x8D => { // STA Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                memory.write(addr, self.a);
            }

            0x9D => { // STA Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                memory.write(addr, self.a);
                // Optional: add cycle penalty if page crossed
            }

            0x99 => { // STA Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                memory.write(addr, self.a);
                // Optional: add cycle penalty if page crossed
            }

            0x81 => { // STA (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                memory.write(addr, self.a);
            }

            0x91 => { // STA (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                memory.write(addr, self.a);
                // Optional: cycle penalty on page cross
            }

            // STX instructions
            0x86 => { // STX Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                memory.write(addr, self.x);
            }

            0x96 => { // STX Zero Page,Y
                let addr = memory.read(self.pc).wrapping_add(self.y) as u16;
                self.pc = self.pc.wrapping_add(1);
                memory.write(addr, self.x);
            }

            0x8E => { // STX Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                memory.write(addr, self.x);
            }

            // STY instructions
            0x84 => { // STY Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                memory.write(addr, self.y);
            }

            0x94 => { // STY Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                memory.write(addr, self.y);
            }

            0x8C => { // STY Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                memory.write(addr, self.y);
            }

            // ------ TRANSFER INSTRUCTIONS ------
            0xAA => { // TAX (Transfer A to X)
                self.x = self.a;
                self.update_zero_and_negative_flags(self.x);
            }

            0xA8 => { // TAY (Transfer A to Y)
                self.y = self.a;
                self.update_zero_and_negative_flags(self.y);
            }

            0xBA => { // TSX (Transfer SP to X)
                self.x = self.sp;
                self.update_zero_and_negative_flags(self.x);
            }

            0x8A => { // TXA (Transfer X to A)
                self.a = self.x;
                self.update_zero_and_negative_flags(self.a);
            }

            0x9A => { // TXS (Transfer X to SP)
                self.sp = self.x;
                // Note: TXS does NOT update any flags
            }

            0x98 => { // TYA (Transfer Y to A)
                self.a = self.y;
                self.update_zero_and_negative_flags(self.a);
            }

            // stack operations
            // ----- PHA, PHP, PLA, PLP Instructions -----
            0x48 => { // PHA (Push Accumulator)
                self.push_u8(memory, self.a);
            }

            0x08 => { // PHP (Push Processor Status)
                // Push status with Break flag and bit 5 set
                let status = self.status | 0b0011_0000; // Set bits 4 and 5
                self.push_u8(memory, status);
            }

            0x68 => { // PLA (Pull Accumulator)
                self.sp = self.sp.wrapping_add(1);
                let addr = 0x0100 | self.sp as u16;
                self.a = memory.read(addr);
                self.update_zero_and_negative_flags(self.a);
            }

            0x28 => { // PLP (Pull Processor Status)
                self.sp = self.sp.wrapping_add(1);
                let addr = 0x0100 | self.sp as u16;
                let status = memory.read(addr);
                // Note: Break flag and bit 5 are ignored when pulled
                self.status = (status & !0b0011_0000) | (self.status & 0b0011_0000);
                // Alternative implementation that properly handles all flags:
                // self.status = (status & 0b1100_1111) | 0b0010_0000; // Clear bits 4 and 5, set bit 5
            }

            0x69 => { // ADC Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.adc(memory, operand);
            }

            0x65 => { // ADC Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.adc(memory, operand);
            }

            0x75 => { // ADC Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.adc(memory, operand);
            }

            0x6D => { // ADC Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.adc(memory, operand);
            }

            0x7D => { // ADC Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                self.adc(memory, operand);
                // Optional: add cycle penalty if page crossed
            }

            0x79 => { // ADC Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.adc(memory, operand);
                // Optional: add cycle penalty if page crossed
            }

            0x61 => { // ADC (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.adc(memory, operand);
            }

            0x71 => { // ADC (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.adc(memory, operand);
                // Optional: cycle penalty on page cross
            }

            // SBC instructions
            0xE9 => { // SBC Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.sbc(memory, operand);
            }

            0xE5 => { // SBC Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.sbc(memory, operand);
            }

            0xF5 => { // SBC Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.sbc(memory, operand);
            }

            0xED => { // SBC Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.sbc(memory, operand);
            }

            0xFD => { // SBC Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                self.sbc(memory, operand);
                // Optional: add cycle penalty if page crossed
            }

            0xF9 => { // SBC Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.sbc(memory, operand);
                // Optional: add cycle penalty if page crossed
            }

            0xE1 => { // SBC (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.sbc(memory, operand);
            }

            0xF1 => { // SBC (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.sbc(memory, operand);
                // Optional: cycle penalty on page cross
            }


            // INC implementations
            0xE6 => { // INC Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let value = memory.read(addr).wrapping_add(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
            }

            0xF6 => { // INC Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let value = memory.read(addr).wrapping_add(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
            }

            0xEE => { // INC Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let value = memory.read(addr).wrapping_add(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
            }

            0xFE => { // INC Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let value = memory.read(addr).wrapping_add(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
                // Optional: add cycle penalty if page crossed
            }

            // INX implementation
            0xE8 => { // INX (Increment X Register)
                self.x = self.x.wrapping_add(1);
                self.update_zero_and_negative_flags(self.x);
            }

            // INY implementation
            0xC8 => { // INY (Increment Y Register)
                self.y = self.y.wrapping_add(1);
                self.update_zero_and_negative_flags(self.y);
            }

            // DEC implementations
            0xC6 => { // DEC Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let value = memory.read(addr).wrapping_sub(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
            }

            0xD6 => { // DEC Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let value = memory.read(addr).wrapping_sub(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
            }

            0xCE => { // DEC Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let value = memory.read(addr).wrapping_sub(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
            }

            0xDE => { // DEC Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let value = memory.read(addr).wrapping_sub(1);
                memory.write(addr, value);
                self.update_zero_and_negative_flags(value);
                // Optional: add cycle penalty if page crossed
            }

            // DEX implementation (to complement DEC)
            0xCA => { // DEX (Decrement X Register)
                self.x = self.x.wrapping_sub(1);
                self.update_zero_and_negative_flags(self.x);
            }

            // DEY implementation (to complement DEC)
            0x88 => { // DEY (Decrement Y Register)
                self.y = self.y.wrapping_sub(1);
                self.update_zero_and_negative_flags(self.y);
            }

            // AND
            0x29 => { // AND Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.and(operand);
            }

            0x25 => { // AND Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.and(operand);
            }

            0x35 => { // AND Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.and(operand);
            }

            0x2D => { // AND Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.and(operand);
            }

            0x3D => { // AND Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                self.and(operand);
                // Optional: add cycle penalty if page crossed
            }

            0x39 => { // AND Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.and(operand);
                // Optional: add cycle penalty if page crossed
            }

            0x21 => { // AND (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.and(operand);
            }

            0x31 => { // AND (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.and(operand);
                // Optional: cycle penalty on page cross
            }

            0x09 => { // ORA Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.ora(operand);
            }

            0x05 => { // ORA Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.ora(operand);
            }

            0x15 => { // ORA Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.ora(operand);
            }

            0x0D => { // ORA Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.ora(operand);
            }

            0x1D => { // ORA Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                self.ora(operand);
                // Optional: add cycle penalty if page crossed
            }

            0x19 => { // ORA Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.ora(operand);
                // Optional: add cycle penalty if page crossed
            }

            0x01 => { // ORA (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.ora(operand);
            }

            0x11 => { // ORA (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.ora(operand);
                // Optional: cycle penalty on page cross
            }

            0x49 => { // EOR Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.eor(operand);
            }

            0x45 => { // EOR Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.eor(operand);
            }

            0x55 => { // EOR Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.eor(operand);
            }

            0x4D => { // EOR Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.eor(operand);
            }

            0x5D => { // EOR Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                self.eor(operand);
                // Optional: add cycle penalty if page crossed
            }

            0x59 => { // EOR Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.eor(operand);
                // Optional: add cycle penalty if page crossed
            }

            0x41 => { // EOR (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.eor(operand);
            }

            0x51 => { // EOR (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.eor(operand);
                // Optional: cycle penalty on page cross
            }

            // BIT
            0x24 => { // BIT Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.bit(memory, operand);
            }

            0x2C => { // BIT Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.bit(memory, operand);
            }


            // ASL
            0x0A => { // ASL Accumulator
                self.a = self.asl(memory, self.a, true);
            }

            0x06 => { // ASL Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.asl(memory, operand, false);
                memory.write(addr, result);
            }

            0x16 => { // ASL Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.asl(memory, operand, false);
                memory.write(addr, result);
            }

            0x0E => { // ASL Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                let result = self.asl(memory, operand, false);
                memory.write(addr, result);
            }

            0x1E => { // ASL Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                let result = self.asl(memory, operand, false);
                memory.write(addr, result);
                // Optional: add cycle penalty if page crossed
            }

            // LSR
            0x4A => { // LSR Accumulator
                self.a = self.lsr(memory, self.a, true);
            }

            0x46 => { // LSR Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.lsr(memory, operand, false);
                memory.write(addr, result);
            }

            0x56 => { // LSR Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.lsr(memory, operand, false);
                memory.write(addr, result);
            }

            0x4E => { // LSR Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                let result = self.lsr(memory, operand, false);
                memory.write(addr, result);
            }

            0x5E => { // LSR Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                let result = self.lsr(memory, operand, false);
                memory.write(addr, result);
                // Optional: add cycle penalty if page crossed
            }

            0x2A => { // ROL Accumulator
                self.a = self.rol(memory, self.a, true);
            }

            0x26 => { // ROL Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.rol(memory, operand, false);
                memory.write(addr, result);
            }

            0x36 => { // ROL Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.rol(memory, operand, false);
                memory.write(addr, result);
            }

            0x2E => { // ROL Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                let result = self.rol(memory, operand, false);
                memory.write(addr, result);
            }

            0x3E => { // ROL Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                let result = self.rol(memory, operand, false);
                memory.write(addr, result);
                // Optional: add cycle penalty if page crossed
            }

            0x6A => { // ROR Accumulator
                self.a = self.ror(memory, self.a, true);
            }

            0x66 => { // ROR Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.ror(memory, operand, false);
                memory.write(addr, result);
            }

            0x76 => { // ROR Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                let result = self.ror(memory, operand, false);
                memory.write(addr, result);
            }

            0x6E => { // ROR Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                let result = self.ror(memory, operand, false);
                memory.write(addr, result);
            }

            0x7E => { // ROR Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                let result = self.ror(memory, operand, false);
                memory.write(addr, result);
                // Optional: add cycle penalty if page crossed
            }

            0xC9 => { // CMP Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.cmp(operand);
            }

            0xC5 => { // CMP Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.cmp(operand);
            }

            0xD5 => { // CMP Zero Page,X
                let addr = memory.read(self.pc).wrapping_add(self.x) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.cmp(operand);
            }

            0xCD => { // CMP Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.cmp(operand);
            }

            0xDD => { // CMP Absolute,X
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                let operand = memory.read(addr);
                self.cmp(operand);
                // Optional: add cycle penalty if page crossed
            }

            0xD9 => { // CMP Absolute,Y
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.cmp(operand);
                // Optional: add cycle penalty if page crossed
            }

            0xC1 => { // CMP (Indirect,X)
                let base = memory.read(self.pc).wrapping_add(self.x);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.cmp(operand);
            }

            0xD1 => { // CMP (Indirect),Y
                let base = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let lo = memory.read(base as u16) as u16;
                let hi = memory.read(base.wrapping_add(1) as u16) as u16;
                let addr = ((hi << 8) | lo).wrapping_add(self.y as u16);
                let operand = memory.read(addr);
                self.cmp(operand);
                // Optional: cycle penalty on page cross
            }

            // CPX instructions
            0xE0 => { // CPX Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.cpx(memory, operand);
            }

            0xE4 => { // CPX Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.cpx(memory, operand);
            }

            0xEC => { // CPX Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.cpx(memory, operand);
            }

            // CPY instructions
            0xC0 => { // CPY Immediate
                let operand = memory.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                self.cpy(memory, operand);
            }

            0xC4 => { // CPY Zero Page
                let addr = memory.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let operand = memory.read(addr);
                self.cpy(memory, operand);
            }

            0xCC => { // CPY Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let addr = (hi << 8) | lo;
                let operand = memory.read(addr);
                self.cpy(memory, operand);
            }

            // JMP implementation
            0x4C => { // JMP Absolute
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                self.pc = (hi << 8) | lo;
                // Note: Don't increment PC as we're jumping
            }

            0x6C => { // JMP Indirect
                let addr_lo = memory.read(self.pc) as u16;
                let addr_hi = memory.read(self.pc.wrapping_add(1)) as u16;
                let addr = (addr_hi << 8) | addr_lo;
                
                // 6502 indirect jump has a bug with page boundaries:
                // It doesn't carry over to the next page when fetching the high byte
                let lo = memory.read(addr) as u16;
                let hi = if (addr & 0xFF) == 0xFF {
                    // Page boundary bug - high byte is fetched from same page
                    memory.read(addr & 0xFF00) as u16
                } else {
                    memory.read(addr.wrapping_add(1)) as u16
                };
                
                self.pc = (hi << 8) | lo;
                // Note: Don't increment PC as we're jumping
            }

            0x20 => { // JSR Absolute
                // Read the target address first
                let lo = memory.read(self.pc) as u16;
                let hi = memory.read(self.pc.wrapping_add(1)) as u16;
                let target_addr = (hi << 8) | lo;
                
                // Push return address (PC + 1) onto stack
                // JSR pushes the address of the last byte of the instruction
                let return_addr = self.pc.wrapping_add(1);
                self.push_u16(memory, return_addr);
                
                // Jump to the target address
                self.pc = target_addr;
            }

            0x60 => { // RTS (Return from Subroutine)
                // Pull return address from stack
                let return_addr = self.pull_u16(memory);
                
                // Set PC to return address + 1 (corrects the +2 from JSR)
                self.pc = return_addr.wrapping_add(1);
                
                // Takes 6 cycles total:
                // 1. Fetch opcode
                // 2. Read next opcode (discarded)
                // 3. Pull low byte from stack
                // 4. Pull high byte from stack
                // 5-6. Internal PC increment
            }

            // ALL BRANCH INSTRUCTIONS:
            // BEQ - Branch if Equal (Zero flag set)
            0xF0 => { // BEQ Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & ZERO_FLAG) != 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BNE - Branch if Not Equal (Zero flag clear)
            0xD0 => { // BNE Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & ZERO_FLAG) == 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BCS - Branch if Carry Set (Carry flag set)
            0xB0 => { // BCS Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & CARRY_FLAG) != 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BCC - Branch if Carry Clear (Carry flag clear)
            0x90 => { // BCC Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & CARRY_FLAG) == 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BMI - Branch if Minus (Negative flag set)
            0x30 => { // BMI Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & NEGATIVE_FLAG) != 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BPL - Branch if Plus/Positive (Negative flag clear)
            0x10 => { // BPL Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & NEGATIVE_FLAG) == 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BVS - Branch if Overflow Set (Overflow flag set)
            0x70 => { // BVS Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & OVERFLOW_FLAG) != 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }

            // BVC - Branch if Overflow Clear (Overflow flag clear)
            0x50 => { // BVC Relative
                let offset = memory.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                
                if (self.status & OVERFLOW_FLAG) == 0 {
                    // Branch taken - add 1 cycle for branch taken
                    let target = self.pc.wrapping_add((offset as i16) as u16);
                    
                    // Add 1 more cycle if page boundary crossed
                    if (self.pc & 0xFF00) != (target & 0xFF00) {
                        // Page boundary crossed - add extra cycle
                    }
                    
                    self.pc = target;
                }
            }


            // INTERRUPT HANDLING, MAY HAVE ERRORS
            0x00 => { // BRK (Force Interrupt)
                self.pc = self.pc.wrapping_add(1); // Skip next byte (BRK padding)
                self.push_u16(memory, self.pc);
                // Push status with Break flag set
                self.push_u8(memory, self.status | 0b00110000); // Set B and unused flags
                self.status |= 0b00000100; // Set Interrupt Disable flag
                self.pc = memory.read_u16(0xFFFE); // Jump to IRQ/BRK vector
            }

            0x40 => { // RTI (Return from Interrupt)
                self.pull_status(memory);
                self.pc = self.pull_u16(memory);
            }

            0xEA => { // NOP (No Operation)
                // Does nothing
            }

            // Flag manipulation instructions
            0x18 => { // CLC (Clear Carry)
                self.status &= 0b11111110;
            }

            0x38 => { // SEC (Set Carry)
                self.status |= 0b00000001;
            }

            0xD8 => { // CLD (Clear Decimal)
                self.status &= 0b11110111;
            }

            0xF8 => { // SED (Set Decimal)
                self.status |= 0b00001000;
            }

            0x58 => { // CLI (Clear Interrupt Disable)
                self.status &= 0b11111011;
            }

            0x78 => { // SEI (Set Interrupt Disable)
                self.status |= 0b00000100;
            }

            0xB8 => { // CLV (Clear Overflow)
                self.status &= 0b10111111;
            }

            // Additional NOP variants (do nothing but take cycles)
            0x1A => { /* NOP */ }
            0x3A => { /* NOP */ }
            0x5A => { /* NOP */ }
            0x7A => { /* NOP */ }
            0xDA => { /* NOP */ }
            0xFA => { /* NOP */ }
            0x80 => { /* NOP (immediate) */ self.pc += 1; }
            0x82 => { /* NOP (immediate) */ self.pc += 1; }
            0x89 => { /* NOP (immediate) */ self.pc += 1; }
            0xC2 => { /* NOP (immediate) */ self.pc += 1; }
            0xE2 => { /* NOP (immediate) */ self.pc += 1; }
            0x04 => { /* NOP (zeropage) */ self.pc += 1; }
            0x44 => { /* NOP (zeropage) */ self.pc += 1; }
            0x64 => { /* NOP (zeropage) */ self.pc += 1; }
            0x14 => { /* NOP (zeropage,X) */ self.pc += 1; }
            0x34 => { /* NOP (zeropage,X) */ self.pc += 1; }
            0x54 => { /* NOP (zeropage,X) */ self.pc += 1; }
            0x74 => { /* NOP (zeropage,X) */ self.pc += 1; }
            0xD4 => { /* NOP (zeropage,X) */ self.pc += 1; }
            0xF4 => { /* NOP (zeropage,X) */ self.pc += 1; }
            0x0C => { /* NOP (absolute) */ self.pc += 2; }
            0x1C => { /* NOP (absolute,X) */ self.pc += 2; }
            0x3C => { /* NOP (absolute,X) */ self.pc += 2; }
            0x5C => { /* NOP (absolute,X) */ self.pc += 2; }
            0x7C => { /* NOP (absolute,X) */ self.pc += 2; }
            0xDC => { /* NOP (absolute,X) */ self.pc += 2; }
            0xFC => { /* NOP (absolute,X) */ self.pc += 2; }

            _ => {
                let log_line = format!("Unimplemented opcode: {:02X} at PC: {:04X}\n", opcode, self.pc - 1);
                let hex_line = format!("{:02X}\n", opcode);
                // debug
                if let Ok(mut file) = OpenOptions::new()
                    .create(true)           // Create if doesn't exist
                    .append(true)           // Append to end of file
                    .open("unimplemented_opcodes.log")
                {
                    let _ = file.write_all(hex_line.as_bytes());
                }
                println!("{}", log_line);
               

            }
        }
    }
}