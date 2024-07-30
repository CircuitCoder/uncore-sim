use std::{cell::RefCell, collections::{HashMap, VecDeque}, ffi::CString, path::Path, rc::Rc};

use crate::drain::Drain;

pub struct MemReq<const WIDTH: usize> {
  pub id: usize,
  pub addr: u64,
  pub wbe: [bool; WIDTH],
  pub wdata: [u8; WIDTH],
}

pub struct MemResp<const WIDTH: usize> {
  pub id: usize,
  pub rdata: [u8; WIDTH],
}

pub trait DelaySimulator {
  fn tick(&mut self);
  fn push(&mut self, addr: u64, is_write: bool);
  fn pop(&mut self) -> Option<u64>;
}

#[derive(Default)]
pub struct NoDelay {
  queue: VecDeque<u64>,
}

impl DelaySimulator for NoDelay {
  fn tick(&mut self) {}
  fn push(&mut self, addr: u64, _is_write: bool) {
    self.queue.push_back(addr);
  }

  fn pop(&mut self) -> Option<u64> {
    self.queue.pop_front()
  }
}

struct AddrProgress {
  sent: u64,
  recv: u64,
  is_write: bool,
}

impl AddrProgress {
  fn next_send(&self, base: u64, transfer: u64) -> u64 {
    base + transfer * self.sent
  }
}

struct Progress<const WIDTH: usize> {
  transfer_width: u64,

  progress: HashMap<u64, AddrProgress>,
  done: VecDeque<u64>,
}

impl<const WIDTH: usize> Default for Progress<WIDTH> {
  fn default() -> Self {
    Progress {
      transfer_width: WIDTH as u64,
      progress: HashMap::new(),
      done: VecDeque::new(),
    }
  }
}

impl<const WIDTH: usize> Progress<WIDTH> {
  fn add(&mut self, addr: u64, is_write: bool) {
    assert_eq!(addr % (WIDTH as u64), 0);
    assert!(self.progress.insert(addr, AddrProgress {
      sent: 0,
      recv: 0,
      is_write,
    }).is_some());
  }

  fn step(&mut self, addr: u64) {
    let aligned = addr - addr % WIDTH as u64;
    let multiplicity = self.multiplicity();
    match self.progress.entry(aligned) {
        std::collections::hash_map::Entry::Occupied(mut o) => {
          let prog = o.get_mut();
          assert_eq!(aligned + prog.recv * self.transfer_width, addr); // Sequential response
          if prog.recv == multiplicity - 1 {
            o.remove();
            self.done.push_back(aligned);
          } else {
            prog.recv += 1;
          }
        }
        std::collections::hash_map::Entry::Vacant(_) => panic!("Unexpected memory response"),
    }
  }

  fn pop(&mut self) -> Option<u64> {
    self.done.pop_front()
  }

  fn multiplicity(&self) -> u64 {
    WIDTH as u64 / self.transfer_width
  }
}

pub struct DRAMSim<const WIDTH: usize> {
  sys: dramsim3::MemorySystem,
  prog: Rc<RefCell<Progress<WIDTH>>>
}

impl<const WIDTH: usize> DRAMSim<WIDTH> {
  pub fn new<Config: AsRef<Path>, Dir: AsRef<Path>>(config: Config, dir: Dir) -> Self {
    let prog: Rc<RefCell<Progress<WIDTH>>> = Default::default();
    let prog_cb = prog.clone();

    let config_cstr = CString::new(config.as_ref().as_os_str().as_encoded_bytes()).unwrap();
    let dir_cstr = CString::new(dir.as_ref().as_os_str().as_encoded_bytes()).unwrap();
    let sys = dramsim3::MemorySystem::new(&config_cstr, &dir_cstr, move |addr, _is_write| {
      prog_cb.borrow_mut().step(addr)
    });

    let transfer_width = sys.bus_bits() * sys.burst_length() / 8;
    prog.borrow_mut().transfer_width = transfer_width as u64;

    DRAMSim { sys, prog }
  }
}

impl<const WIDTH: usize> DelaySimulator for DRAMSim<WIDTH> {
  fn tick(&mut self) {
    self.sys.tick();
    let mut prog = self.prog.borrow_mut();
    let multiplicity = prog.multiplicity();
    let transfer_width = prog.transfer_width;
    for (aligned, addr_prog) in prog.progress.iter_mut() {
      if addr_prog.sent != multiplicity {
        let next_addr = addr_prog.next_send(*aligned, transfer_width);
        if self.sys.can_add(next_addr, addr_prog.is_write) {
          self.sys.add(next_addr, addr_prog.is_write);
          addr_prog.sent += 1;
        }
      }
    }
  }

  fn push(&mut self, addr: u64, is_write: bool) {
    self.prog.borrow_mut().add(addr, is_write);
  }

  fn pop(&mut self) -> Option<u64> {
    self.prog.borrow_mut().pop()
  }
}

pub struct Mem<D: DelaySimulator, const WIDTH: usize> {
  sim: D,
  content: HashMap<u64, [u8; WIDTH]>,
  inflights: HashMap<u64, usize>,
}

impl<D: DelaySimulator, const WIDTH: usize> Mem<D, WIDTH> {
  pub fn new(sim: D) -> Self {
    Mem {
      sim,
      content: HashMap::new(),
      inflights: HashMap::new(),
    }
  }
}

impl<D: DelaySimulator, const WIDTH: usize> Drain for Mem<D, WIDTH> {
  type Req = MemReq<WIDTH>;
  type Resp = MemResp<WIDTH>;
  fn tick(&mut self) {
    self.sim.tick();
  }

  fn push(&mut self, req: MemReq<WIDTH>) {
    if self.inflights.insert(req.addr, req.id).is_some() {
      panic!("Duplicated inflight memory requests");
    }
    self.sim.push(req.addr, req.wbe.contains(&true));
    match self.content.entry(req.addr) {
      std::collections::hash_map::Entry::Occupied(mut o) => {
        for (c, (w, be)) in o.get_mut().iter_mut().zip(req.wdata.iter().zip(req.wbe.iter())) {
          if *be { *c = *w; }
        }
      },
      std::collections::hash_map::Entry::Vacant(v) => {
        let mut buf = [0; WIDTH];
        for (c, (w, be)) in buf.iter_mut().zip(req.wdata.iter().zip(req.wbe.iter())) {
          if *be { *c = *w; }
        }
        v.insert(buf);
      },
    }
  }

  fn pop(&mut self) -> Option<MemResp<WIDTH>> {
    self.sim.pop().map(|addr| {
      let rdata = self.content.get(&addr).cloned().unwrap_or([0; WIDTH]);
      let id = self.inflights.remove(&addr).expect("Unexpected memory response");
      MemResp {
        id,
        rdata,
      }
    })
  }
}