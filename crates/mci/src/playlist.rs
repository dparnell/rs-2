//! The memory playlist processor (MCI_OPEN_PLAYLIST): an array of
//! instructions in the application's memory, each an operation code and
//! three operands (16 bytes). The processor runs the control flow itself
//! and hands out what the device has to act on: data buffers to play,
//! messages, cue points and semaphores. The application may change the
//! playlist while it plays, so every instruction is read when it runs.

use alloc::vec::Vec;

pub const DATA_OPERATION: u32 = 0;
pub const BRANCH_OPERATION: u32 = 1;
pub const LOOP_OPERATION: u32 = 2;
pub const CALL_OPERATION: u32 = 3;
pub const RETURN_OPERATION: u32 = 4;
pub const EXIT_OPERATION: u32 = 5;
pub const NOP_OPERATION: u32 = 6;
pub const MESSAGE_OPERATION: u32 = 7;
pub const CUEPOINT_OPERATION: u32 = 8;
pub const SEMWAIT_OPERATION: u32 = 9;
pub const SEMPOST_OPERATION: u32 = 10;

/// CALL instructions nest up to 20 levels.
const MAX_CALLS: usize = 20;
/// Control instructions run in a row without producing anything before
/// the playlist counts as stuck (a branch to itself).
const MAX_IDLE: u32 = 10_000;

/// Where the playlist lives.
pub trait Memory {
    fn read(&self, addr: u32) -> u32;
    fn write(&mut self, addr: u32, v: u32);
}

/// What the device has to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Play `len` bytes at `addr`; `pos_at` is the address of operand 3,
    /// where the device records how many bytes it has used.
    Data { addr: u32, len: u32, pos_at: u32 },
    /// MESSAGE_OPERATION: MM_MCIPLAYLISTMESSAGE with this value.
    Message(u32),
    /// CUEPOINT_OPERATION: MM_MCICUEPOINT with the user parameter, at
    /// this MMTIME offset into the next data.
    Cuepoint { user: u32, offset: u32 },
    SemWait { sem: u32, timeout: u32, reset: bool },
    SemPost(u32),
    /// EXIT_OPERATION, a RETURN without a CALL, an unknown operation, or
    /// a playlist that went round without doing anything.
    Exit,
}

#[derive(Debug, Clone, Default)]
pub struct Processor {
    base: u32,
    pc: u32,
    stack: Vec<u32>,
}

impl Processor {
    pub fn new(base: u32) -> Processor {
        Processor { base, pc: 0, stack: Vec::new() }
    }

    /// Back to the first instruction.
    pub fn rewind(&mut self) {
        self.pc = 0;
        self.stack.clear();
    }

    /// The instruction that runs next.
    pub fn position(&self) -> u32 {
        self.pc
    }

    fn at(&self, i: u32) -> u32 {
        self.base.wrapping_add(i.wrapping_mul(16))
    }

    /// Run control instructions up to the next thing the device does.
    pub fn next(&mut self, m: &mut dyn Memory) -> Step {
        for _ in 0..MAX_IDLE {
            let a = self.at(self.pc);
            let (op, o1, o2, o3) = (m.read(a), m.read(a + 4), m.read(a + 8), m.read(a + 12));
            let here = self.pc;
            self.pc = here.wrapping_add(1);
            match op {
                DATA_OPERATION => return Step::Data { addr: o1, len: o2, pos_at: a + 12 },
                BRANCH_OPERATION => self.pc = o2,
                LOOP_OPERATION => {
                    if o3 < o1 {
                        m.write(a + 12, o3 + 1);
                    } else {
                        m.write(a + 12, 0);
                        self.pc = o2;
                    }
                }
                CALL_OPERATION => {
                    if self.stack.len() >= MAX_CALLS {
                        return Step::Exit;
                    }
                    self.stack.push(self.pc);
                    self.pc = o2;
                }
                RETURN_OPERATION => match self.stack.pop() {
                    Some(r) => self.pc = r,
                    None => return Step::Exit,
                },
                NOP_OPERATION => {}
                MESSAGE_OPERATION => return Step::Message(o2),
                CUEPOINT_OPERATION => return Step::Cuepoint { user: o1, offset: o2 },
                SEMWAIT_OPERATION => return Step::SemWait { sem: o1, timeout: o2, reset: o3 != 0 },
                SEMPOST_OPERATION => return Step::SemPost(o1),
                _ => {
                    self.pc = here;
                    return Step::Exit;
                }
            }
        }
        Step::Exit
    }
}
