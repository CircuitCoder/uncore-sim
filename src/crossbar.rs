use std::{collections::BTreeMap, fmt::Display};

use crate::drain::Drain;

pub trait Addr : Eq + Ord + Copy + Display {}

pub trait Routable<A: Addr> {
  fn addr(&self) -> A;
}

pub struct Crossbar<A: Addr, Req: Routable<A>, Resp> {
  children: BTreeMap<A, (A, Box<dyn Drain<Req = Req, Resp = Resp>>)>
}

impl<A: Addr, Req: Routable<A>, Resp> Crossbar<A, Req, Resp> {
  pub fn new() -> Crossbar<A, Req, Resp> {
    Crossbar {
      children: BTreeMap::new(),
    }
  }
  pub fn with(mut self, start: A, end: A, inner: Box<dyn Drain<Req = Req, Resp = Resp>>) -> Crossbar<A, Req, Resp> {
    self.children.insert(start, (end, inner));
    self
  }
}

impl<A: Addr, Req: Routable<A>, Resp> Drain for Crossbar<A, Req, Resp> {
  type Req = Req;
  type Resp = Resp;

  fn tick(&mut self) {
    for (_, (_, child)) in self.children.iter_mut() {
      child.tick();
    }
  }
  
  fn push(&mut self, req: Self::Req) {
    let addr = req.addr();
    let lb = self.children.range_mut(addr..).next();
    if lb.is_none() || lb.as_ref().unwrap().1.0 <= addr {
      panic!("Out-of-range request address: {}", addr);
    }

    lb.unwrap().1.1.push(req);
  }

  fn pop(&mut self) -> Option<Resp> {
    for (_, (_, child)) in self.children.iter_mut() {
      let try_pop = child.pop();
      if try_pop.is_some() {
        return try_pop;
      }
    }
    None
  }
}