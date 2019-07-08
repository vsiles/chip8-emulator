extern crate minifb;

use std::env;
use std::fs;
use std::time::SystemTime;

const CHIP8_WIDTH : usize = 64;
const CHIP8_HEIGHT : usize = 32;
const FONT_BASE_ADDR : usize = 0x0;
const ROM_BASE_ADDR : usize = 0x200;

const FONT : [u8; 80] = [
    0xF0, 0x90, 0x90, 0x90, 0xF0, // 0
    0x20, 0x60, 0x20, 0x20, 0x70, // 1
    0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
    0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
    0x90, 0x90, 0xF0, 0x10, 0x10, // 4
    0xF0, 0x80, 0xF0, 0x10, 0xF0, // 5
    0xF0, 0x80, 0xF0, 0x90, 0xF0, // 6
    0xF0, 0x10, 0x20, 0x40, 0x40, // 7
    0xF0, 0x90, 0xF0, 0x90, 0xF0, // 8
    0xF0, 0x90, 0xF0, 0x10, 0xF0, // 9
    0xF0, 0x90, 0xF0, 0x90, 0x90, // A
    0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
    0xF0, 0x80, 0x80, 0x80, 0xF0, // C
    0xE0, 0x90, 0x90, 0x90, 0xE0, // D
    0xF0, 0x80, 0xF0, 0x80, 0xF0, // E
    0xF0, 0x80, 0xF0, 0x80, 0x80  // F
];

// const FONT_WIDTH : usize = 4;
const FONT_HEIGHT : usize = 5;

#[inline(always)]
fn build_opcode(upper: u8, lower: u8) -> u16 {
    (upper as u16) << 8 | (lower as u16)
}

#[inline(always)]
fn extract_address(upper: u8, lower: u8) -> usize {
    build_opcode(upper & 0x0f, lower) as usize
}

#[inline(always)]
fn extract_lower(byte: u8) -> u8 {
    byte & 0x0f
}

#[inline(always)]
fn extract_upper(byte: u8) -> u8 {
    byte & 0xf0
}

#[inline(always)]
fn next(pc: usize) -> usize {
    pc + 2
}

struct State {
    // Registers
    v_regs : [u8; 16],
    i_reg : usize,
    pc : usize,
    sp: usize,
    stack: [usize; 24],
    delay_timer: u8,
    delay_timer_time: SystemTime,
    sound_timer: u8,
    inputs: [bool; 16],
    memory: [u8; 4096],
    screen: [u8; CHIP8_WIDTH * CHIP8_HEIGHT],
    blocking: bool,
    target_key: usize,
}

// decode current instruction. PC is already updated to the next one
fn decode_instruction(state: &mut State, upper: u8, lower: u8) -> bool {
    let top = extract_upper(upper) >> 4;
    let x = extract_lower(upper) as usize;
    let y = (extract_upper(lower) >> 4) as usize;
    let nn = lower;
    let bot = extract_lower(lower);
    let mut ret = false;

    match top {
        0x0 => if lower == 0xe0 {
            for i in state.screen.iter_mut() {
                *i = 0
            }
            ret = true
        } else if lower == 0xee {
            if state.sp == 0 {
                println!("E: Nothing to pop from the stack")
            } else {
                state.pc = state.stack[state.sp - 1];
                state.sp = state.sp - 1
            }
        } else {
            println!("I: SYS\tRCA 1802")
        },
        0x1 => state.pc = extract_address(upper, lower),
        0x2 => {
            if state.sp >= 24 {
                println!("E: To many nested subroutine calls")
            } else {
                state.stack[state.sp] = state.pc;
                state.sp = state.sp + 1;
                state.pc = extract_address(upper, lower)
            }
        },
        0x3 => {
            if state.v_regs[x] == nn {
                state.pc = next(state.pc)
            }
        },
        0x4 => {
            if state.v_regs[x] != nn {
                state.pc = next(state.pc)
            }
        },
        0x5 => {
            if bot == 0 {
                if state.v_regs[x] == state.v_regs[y] {
                    state.pc = next(state.pc)
                }
            } else {
                println!("E: Unsupported opcode {:04x}", build_opcode(upper, lower))
            }
        },
        0x6 => state.v_regs[x] = nn,
        0x7 => {
            let vx = state.v_regs[x];
            state.v_regs[x] = vx.overflowing_add(nn).0
        },
        0x8 => {
            match bot {
                0x0 => state.v_regs[x] = state.v_regs[y],
                0x1 => {
                    let vx = state.v_regs[x];
                    state.v_regs[x] = vx | state.v_regs[y]
                },
                0x2 => {
                    let vx = state.v_regs[x];
                    state.v_regs[x] = vx & state.v_regs[y]
                },
                0x3 => {
                    let vx = state.v_regs[x];
                    state.v_regs[x] = vx ^ state.v_regs[y]
                },
                0x4 => {
                    let vx = state.v_regs[x];
                    let vy = state.v_regs[y];
                    let (nvx, carry) = vx.overflowing_add(vy);
                    state.v_regs[0xf] = if carry { 1 } else { 0 };
                    state.v_regs[x] = nvx
                },
                0x5 => {
                    let vx = state.v_regs[x];
                    let vy = state.v_regs[y];
                    let (nvx, borrow) = vx.overflowing_sub(vy);
                    state.v_regs[0xf] = if borrow { 0 } else { 1 };
                    state.v_regs[x] = nvx
                },
                0x6 => {
                    let vx = state.v_regs[x];
                    state.v_regs[0xf] = vx & 1;
                    state.v_regs[x] = vx >> 1
                },
                0x7 => {
                    let vx = state.v_regs[x];
                    let vy = state.v_regs[y];
                    let (nvx, borrow) = vy.overflowing_sub(vx);
                    state.v_regs[0xf] = if borrow { 0 } else { 1 };
                    state.v_regs[x] = nvx
                },
                0xe => {
                    let vx = state.v_regs[x];
                    state.v_regs[0xf] = vx >> 7;
                    state.v_regs[x] = vx << 1
                },
                _ => println!("E: Unsupported opcode {:04x}", build_opcode(upper, lower))
            }
        }
        0x9 => {
            if bot == 0 {
                if state.v_regs[x] != state.v_regs[y] {
                    state.pc = next(state.pc);
                }
            } else {
                println!("E: Unsupported opcode {:04x}", build_opcode(upper, lower))
            }
        },
        0xa => state.i_reg = extract_address(upper, lower),
        0xb => state.pc = (state.v_regs[0] as usize) + extract_address(upper, lower),
        // TODO add randomness, see
        // https://rust-lang-nursery.github.io/rust-cookbook/algorithms/randomness.html
        0xc => state.v_regs[x] = /*random*/ 42 & nn,
        0xd => {
            // draws a pixel at location x y, with 8 bit width and n bith height
            // data comes from I (which remains unchanged)
            // VF is set to 1 if any screen pixel is flipped from set to unset
            let n = bot as usize;
            let vx = state.v_regs[x] as usize;
            let vy = state.v_regs[y] as usize;
            state.v_regs[0xf] = 0; // TODO: not sure
            for h in 0..n {
                let new_byte = state.memory[state.i_reg + h];
                for w in 0..8 {
                    if (new_byte & (0x80 >> w)) != 0 {
                        let old_pixel = state.screen[vx + w + CHIP8_WIDTH * (vy + h)];
                        if old_pixel > 0 {
                            state.v_regs[0xf] = 1;
                        }
                        state.screen[vx + w + CHIP8_WIDTH * (vy + h)]  = old_pixel ^ 1
                    }
                }
            }
            ret = true
        }
        0xe => {
            let key = state.v_regs[x] as usize;
            if lower == 0x9e {
                if state.inputs[key] {
                    state.pc = next(state.pc)
                }
            } else if lower == 0xa1 {
                if !state.inputs[key] {
                    state.pc = next(state.pc)
                }
            } else {
                println!("E: Unsupported opcode {:04x}", build_opcode(upper, lower))
            }
        },
        0xf => {
            match lower {
                0x07 => state.v_regs[x] = state.delay_timer,
                0x0a => {
                    state.blocking = true;
                    state.target_key = x
                },
                0x15 => {
                    state.delay_timer = state.v_regs[x];
                    state.delay_timer_time = SystemTime::now()
                },
                0x18 => state.sound_timer = state.v_regs[x],
                0x1e => {
                    let i = state.i_reg;
                    state.i_reg = i + (state.v_regs[x] as usize)
                },
                0x29 => {
                    // We only support default font 0..F
                    let vx = state.v_regs[x] as usize & 0xf;
                    state.i_reg = FONT_BASE_ADDR + vx * FONT_HEIGHT;
                },
                0x33 => {
                    let vx = state.v_regs[x];
                    let v100 = vx / 100;
                    let v10 = (vx % 100) / 10;
                    let v1 = vx % 10;
                    let i = state.i_reg;

                    state.memory[i + 0] = v100;
                    state.memory[i + 1] = v10;
                    state.memory[i + 2] = v1
                },
                0x55 => {
                    let i = state.i_reg;
                    for v in 0..(x + 1) {
                        state.memory[i + v] = state.v_regs[v]
                    }
                },
                0x65 => {
                    let i = state.i_reg;
                    for v in 0..(x + 1) {
                        state.v_regs[v] = state.memory[i + v]
                    }
                }
                _ => println!("E: Unsupported opcode {:04x}", build_opcode(upper, lower))
            }
        },
        _ => println!("E: Unsupported opcode {:04x}", build_opcode(upper, lower))
    }
    ret
}


////// GFX //////// GFX //////////

use minifb::{Window, Key, /* Scale, */ WindowOptions};

const BLOCK_SIZE : usize = 10;
const WIDTH: usize = CHIP8_WIDTH * BLOCK_SIZE;
const HEIGHT: usize = CHIP8_HEIGHT * BLOCK_SIZE;

fn main() {
    let args: Vec<String> = env::args().collect();
    println!("Command line arguments: {:?}\n", args);

    // support only a single filename on the CL for the moment
    // TODO: support nice flags
    if args.len() < 3 {
        println!("Not enough arguments.");
        println!("Usage: {} filename speed", args[0]);
        std::process::exit(1);
    }

    let filename = &args[1];
    let speed : u128 = args[2].parse().unwrap();
    println!("SPEED = {}", speed);
    let rom = fs::read(filename).expect("Can't read input file");
    let rom_len = rom.len();

    if rom_len % 2 == 1 {
        println!("Invalid ROM, need even number of bytes. Found {}", rom_len);
        std::process::exit(2);
    }

    let mut state = State{
        v_regs : [0; 16],
        i_reg : 0,
        pc : ROM_BASE_ADDR,
        sp: 0,
        stack: [0; 24],
        delay_timer: 0,
        delay_timer_time: SystemTime::now(),
        sound_timer: 0,
        inputs: [false; 16],
        memory: [0; 4096],
        screen: [0; CHIP8_WIDTH * CHIP8_HEIGHT],
        blocking: false,
        target_key: 0,
    };

    // allocate cleared screen buffer
    let mut buffer: Vec<u32> = vec![0; WIDTH * HEIGHT];

    // Load font at 0x0
    for i in 0..80 {
        state.memory[i] = FONT[i]
    }

    // Load rom at ROM_BASE_ADDR
    for i in 0..rom_len {
        state.memory[ROM_BASE_ADDR + i] = rom[i]
    }

    // setup red screen
    for i in buffer.iter_mut() {
        *i = 0
    }
    
    let mut window = match Window::new("CHIP8 simple emulator - Press ESC to exit", WIDTH, HEIGHT,
                                       WindowOptions {
                                           // resize: true,
                                           // scale: Scale::X2,
                                           ..WindowOptions::default()
                                       }) {
        Ok(win) => win,
        Err(err) => {
            println!("Unable to create window {}", err);
            return;
        }
    };


    let mut cur = SystemTime::now();
    
    while window.is_open() && !window.is_key_down(Key::Escape) {
        // {
        //     let new_size = window.get_size();
        //     if new_size != size {
        //         size = new_size;
        //         buffer.resize(size.0 * size.1 / 2 / 2, 0);
        //     }
        // }

        window.get_keys().map(|keys| {
            for i in state.inputs.iter_mut() {
                *i = false
            }
            for t in keys {
                match t {
                     Key::Key1 => state.inputs[0x0] = true,
                     Key::Key2 => state.inputs[0x1] = true,
                     Key::Key3 => state.inputs[0x2] = true,
                     Key::Key4 => state.inputs[0x3] = true,
                     Key::Q => state.inputs[0x4] = true,
                     Key::W => state.inputs[0x5] = true,
                     Key::E => state.inputs[0x6] = true,
                     Key::R => state.inputs[0x7] = true,
                     Key::A => state.inputs[0x8] = true,
                     Key::S => state.inputs[0x9] = true,
                     Key::D => state.inputs[0xa] = true,
                     Key::F => state.inputs[0xb] = true,
                     Key::Z => state.inputs[0xc] = true,
                     Key::X => state.inputs[0xd] = true,
                     Key::C => state.inputs[0xe] = true,
                     Key::V => state.inputs[0xf] = true,
                    _ => (),
                }
            }
        });

        // We unwrap here as we want this code to exit if it fails
        window.update_with_buffer(&buffer).unwrap();

        if state.blocking {
            for i in 0..16 {
                if state.inputs[i] {
                    state.v_regs[state.target_key] = i as u8;
                    state.blocking = false
                }
            }
        } else {
            let pc = state.pc;
            let upper = state.memory[pc];
            let lower = state.memory[pc + 1];
            state.pc = next(pc); // update pc before decoding the opcode
            let opcode = build_opcode(upper, lower);
            println!("{:#04x}\t{:04x}", pc, opcode);
            if decode_instruction(&mut state, upper, lower) {
                // Update display
                for y in 0..CHIP8_HEIGHT {
                    for x in 0..CHIP8_WIDTH {
                        let mut color = 0;
                        if state.screen[x + CHIP8_WIDTH * y] != 0 {
                            color = 255 << 8
                        }
                        for h in 0..BLOCK_SIZE {
                            for w in 0..BLOCK_SIZE {
                                let nx = BLOCK_SIZE * x + w;
                                let ny = BLOCK_SIZE * y + h;
                                buffer[nx + WIDTH * ny] = color
                            }
                        }
                    }
                }
            }
        }

        // keep to speed (== 2 -> 500 MHz)
        loop {
            let next = SystemTime::now();
            let ms = next.duration_since(cur).unwrap().as_millis();
            if ms >= speed { break }
        }
        cur = SystemTime::now();

        let delay = state.delay_timer;
        if delay > 0 {
            let ms = cur.duration_since(state.delay_timer_time).unwrap().as_millis();
            if ms >= 17 {
                state.delay_timer = delay - 1;
                state.delay_timer_time = cur
            }
        }
    }
}
