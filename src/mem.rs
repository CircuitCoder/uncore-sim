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
  fn push(&mut self, addr: u64, is_write: bool) -> bool;
  fn pop(&mut self) -> Option<u64>;
}

#[derive(Default)]
pub struct NoDelay {
  queue: VecDeque<u64>,
}

impl DelaySimulator for NoDelay {
  fn tick(&mut self) {}
  fn push(&mut self, addr: u64, _is_write: bool) -> bool {
    self.queue.push_back(addr);
    true
  }
  fn pop(&mut self) -> Option<u64> {
    self.queue.pop_front()
  }
}

pub struct DRAMSim {
  sys: dramsim3::MemorySystem,
  channel: Rc<RefCell<VecDeque<u64>>>
}

impl DRAMSim {
  pub fn new<Config: AsRef<Path>, Dir: AsRef<Path>>(config: Config, dir: Dir) -> Self {
    let channel: Rc<RefCell<VecDeque<u64>>> = Default::default();
    let channel_cb = channel.clone();

    let config_cstr = CString::new(config.as_ref().as_os_str().as_encoded_bytes()).unwrap();
    let dir_cstr = CString::new(dir.as_ref().as_os_str().as_encoded_bytes()).unwrap();
    let sys = dramsim3::MemorySystem::new(&config_cstr, &dir_cstr, move |addr, _is_write| {
      channel_cb.borrow_mut().push_back(addr)
    });
    DRAMSim { sys, channel }
  }
}

impl DelaySimulator for DRAMSim {
  fn tick(&mut self) {
    self.sys.tick()
  }

  fn push(&mut self, addr: u64, is_write: bool) -> bool {
    if !self.sys.can_add(addr, is_write) {
      return false;
    }
    self.sys.add(addr, is_write)
  }

  fn pop(&mut self) -> Option<u64> {
    self.channel.borrow_mut().pop_front()
  }
}

pub struct Mem<D: DelaySimulator, const WIDTH: usize> {
  sim: D,
  content: HashMap<u64, [u8; WIDTH]>,
  pendings: VecDeque<MemReq<WIDTH>>,
  inflights: HashMap<u64, usize>,
}

impl<D: DelaySimulator, const WIDTH: usize> Mem<D, WIDTH> {
  pub fn new(sim: D) -> Self {
    Mem {
      sim,
      content: HashMap::new(),
      pendings: VecDeque::new(),
      inflights: HashMap::new(),
    }
  }
}

impl<D: DelaySimulator, const WIDTH: usize> Drain for Mem<D, WIDTH> {
  type Req = MemReq<WIDTH>;
  type Resp = MemResp<WIDTH>;
  fn tick(&mut self) {
    self.sim.tick();

    while let Some(req) = self.pendings.front() {
      // TODO: check addr alignment
      if self.sim.push(req.addr, req.wbe.contains(&true)) {
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
        self.pendings.pop_front();
      }
    }
  }

  fn push(&mut self, req: MemReq<WIDTH>) {
    if self.inflights.insert(req.addr, req.id).is_some() {
      panic!("Duplicated inflight memory requests");
    }
    self.pendings.push_back(req);
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