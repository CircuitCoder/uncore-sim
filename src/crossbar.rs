use std::{collections::BTreeMap, fmt::Display};

use crate::drain::Drain;

pub trait Addr: Eq + Ord + Copy + Display {}
impl Addr for u64 {}

pub trait Routable<A: Addr> {
    fn addr(&self) -> A;
}

pub struct Crossbar<A: Addr, Req: Routable<A>, Resp> {
    children: BTreeMap<A, (A, Box<dyn Drain<Req = Req, Resp = Resp>>)>,
}

impl<A: Addr, Req: Routable<A>, Resp> Crossbar<A, Req, Resp> {
    pub fn new() -> Crossbar<A, Req, Resp> {
        Crossbar {
            children: BTreeMap::new(),
        }
    }
    pub fn with(
        mut self,
        start: A,
        end: A,
        inner: Box<dyn Drain<Req = Req, Resp = Resp>>,
    ) -> Crossbar<A, Req, Resp> {
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
        let lb = self.children.range_mut(..addr).last();
        if lb.is_none() || lb.as_ref().unwrap().1 .0 <= addr {
            panic!("Out-of-range request address: {}", addr);
        }

        lb.unwrap().1 .1.push(req);
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

#[test]
fn test_multiple_memory() {
    use crate::drain::*;
    use crate::mem::*;
    let mut mem_a: Mem<_, 8> = Mem::new(NoDelay::default());
    let mut mem_b: Mem<_, 8> = Mem::new(NoDelay::default());

    mem_a.tick();
    mem_a.push(MemReq {
        id: 0,
        addr: 0x80000040,
        wbe: [true; 8],
        wdata: [1; 8],
    });
    loop {
        if mem_a.pop().is_some() {
            break;
        }
        mem_a.tick();
    }

    mem_b.tick();
    mem_b.push(MemReq {
        id: 0,
        addr: 0x80002040,
        wbe: [true; 8],
        wdata: [2; 8],
    });
    loop {
        if mem_b.pop().is_some() {
            break;
        }
        mem_b.tick();
    }

    let mut crossbar = Crossbar::new()
        .with(0x80000000, 0x80002000, Box::new(Delay::new(mem_a, 3, 5)))
        .with(0x80002000, 0x80004000, Box::new(Delay::new(mem_b, 4, 2)));

    crossbar.tick();
    crossbar.push(MemReq {
        id: 1,
        addr: 0x80000040,
        wbe: [false; 8],
        wdata: [0; 8],
    });

    crossbar.push(MemReq {
        id: 2,
        addr: 0x80002040,
        wbe: [false; 8],
        wdata: [0; 8],
    });

    let mut popped = 0;
    'outer: loop {
        crossbar.tick();
        while let Some(resp) = crossbar.pop() {
            assert_eq!(resp.rdata, [resp.id as u8; 8]);
            popped += 1;
            if popped == 2 {
                break 'outer;
            }
        }
    }
}

#[test]
#[should_panic]
fn test_multiple_memory_oob() {
    use crate::drain::*;
    use crate::mem::*;
    let mem_a: Mem<_, 8> = Mem::new(NoDelay::default());
    let mem_b: Mem<_, 8> = Mem::new(NoDelay::default());
    let mut crossbar = Crossbar::new()
        .with(0x80000000, 0x80002000, Box::new(Delay::new(mem_a, 3, 5)))
        .with(0x80004000, 0x80008000, Box::new(Delay::new(mem_b, 4, 2)));
    crossbar.push(MemReq {
        id: 0,
        addr: 0x80002000,
        wbe: [false; 8],
        wdata: [0; 8],
    });

    for _ in 0..10 {
        crossbar.tick();
    }
}
