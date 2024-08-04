use std::collections::VecDeque;

pub trait Drain {
    type Req;
    type Resp;
    fn tick(&mut self);
    fn push(&mut self, req: Self::Req);
    fn pop(&mut self) -> Option<Self::Resp>;
}

pub struct Delay<T: Drain> {
    inner: T,

    up_delay: usize,
    down_delay: usize,

    tick: usize,
    downlink: VecDeque<(usize, T::Req)>,
    uplink: VecDeque<(usize, T::Resp)>,
}

impl<T: Drain> Delay<T> {
    pub fn new(inner: T, up_delay: usize, down_delay: usize) -> Delay<T> {
        Delay {
            inner,
            up_delay,
            down_delay,
            tick: 0,
            downlink: VecDeque::new(),
            uplink: VecDeque::new(),
        }
    }
}

impl<T: Drain> Drain for Delay<T> {
    type Req = T::Req;
    type Resp = T::Resp;

    fn tick(&mut self) {
        self.inner.tick();
        self.tick += 1;
        while self.downlink.front().is_some_and(|(t, _)| *t >= self.tick) {
            let (_, req) = self.downlink.pop_front().unwrap();
            self.inner.push(req);
        }

        while let Some(resp) = self.inner.pop() {
            self.uplink.push_back((self.tick + self.up_delay, resp));
        }
    }

    fn push(&mut self, req: Self::Req) {
        self.downlink.push_back((self.tick + self.down_delay, req));
    }

    fn pop(&mut self) -> Option<Self::Resp> {
        if self.uplink.front().is_some_and(|(t, _)| *t >= self.tick) {
            let (_, resp) = self.uplink.pop_front().unwrap();
            Some(resp)
        } else {
            None
        }
    }
}
