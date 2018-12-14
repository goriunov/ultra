// This part is taken from tokio io.
use std::io;
use std::mem;

use futures::{Future, Poll};

use futures::try_ready;

use tokio::prelude::*;

use crate::response;

use http::Response;

pub struct WriteAll<A> {
  state: State<A>,
}

enum State<A> {
  Writing { a: A, buf: Vec<u8>, pos: usize },
  Empty,
}

pub fn write_all<A>(a: A, res: Response<String>) -> WriteAll<A>
where
  A: AsyncWrite,
{
  WriteAll {
    state: State::Writing {
      a: a,
      buf: response::generate_response(res).into_bytes(),
      pos: 0,
    },
  }
}

fn zero_write() -> io::Error {
  io::Error::new(io::ErrorKind::WriteZero, "zero-length write")
}

impl<A> Future for WriteAll<A>
where
  A: AsyncWrite,
{
  type Item = (A);
  type Error = io::Error;

  fn poll(&mut self) -> Poll<(A), io::Error> {
    match self.state {
      State::Writing {
        ref mut a,
        ref buf,
        ref mut pos,
      } => {
        while *pos < buf.len() {
          let n = try_ready!(a.poll_write(&buf[*pos..]));
          *pos += n;
          if n == 0 {
            return Err(zero_write());
          }
        }
      }
      State::Empty => panic!("poll a WriteAll after it's done"),
    }

    match mem::replace(&mut self.state, State::Empty) {
      State::Writing { a, buf, .. } => Ok(a.into()),
      State::Empty => panic!(),
    }
  }
}
