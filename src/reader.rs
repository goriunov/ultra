use super::*;

use std::time::{Duration, Instant};
use tokio::timer::Delay;

#[derive(PartialEq)]
enum ReadState {
  Body,
  Chunk,
  Request,
}

enum ProcessState {
  Empty,
  Ready(ReqResTuple),
  Processing(ReturnFuture),
}

pub struct Reader<T> {
  socket: ReadHalf,
  buffer: BytesMut,
  req_func: OnData,
  body_size: usize,
  read_state: ReadState,
  router_raw: *const T,
  process_state: ProcessState,
  keep_alive_timer: Delay,
}

impl<T> Reader<T>
where
  T: RouterSearch,
{
  pub fn new((socket, write_socket): (ReadHalf, WriteHalf), router: &T) -> Reader<T> {
    Reader {
      socket,
      buffer: BytesMut::with_capacity(1024),
      req_func: OnData::Empty,
      body_size: 0,
      router_raw: router as *const T,
      read_state: ReadState::Request,
      keep_alive_timer: Delay::new(Instant::now() + Duration::from_secs(10)),
      process_state: ProcessState::Ready((
        request::Request::new(),
        response::Response::new(write_socket),
      )),
    }
  }
}

impl<T> Future for Reader<T>
where
  T: RouterSearch,
{
  type Item = ReqResTuple;
  type Error = std::io::Error;

  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
    // dbg!(self.timer.poll());
    loop {
      match std::mem::replace(&mut self.process_state, ProcessState::Empty) {
        ProcessState::Empty => unreachable!(), // this should never be called
        ProcessState::Processing(mut fut) => {
          match fut.poll()? {
            Async::Ready((mut req, res)) => {
              self
                .keep_alive_timer
                .reset(Instant::now() + Duration::from_secs(10));
              // fetch function from request in to the reader for easier execution
              if req.has_function {
                req.has_function = false;
                self.req_func = std::mem::replace(&mut req.on_data, OnData::Empty);
              }

              self.process_state = ProcessState::Ready((req, res));
            }
            Async::NotReady => {
              self.process_state = ProcessState::Processing(fut);
              return Ok(Async::NotReady);
            }
          }
        }
        ProcessState::Ready((mut req, mut res)) => {
          loop {
            // check what reading state we are in
            match self.read_state {
              ReadState::Body => {
                if self.buffer.len() >= self.body_size {
                  // reset state and emit all received data to user
                  self.read_state = ReadState::Request;

                  match &self.req_func {
                    OnData::Function(f) => {
                      req.data = self.buffer.split_to(self.body_size);
                      req.is_last = true;
                      let fut = (f)((req, res));
                      self.process_state = ProcessState::Processing(fut.into_future());
                      break;
                    }
                    OnData::Empty => self.buffer.advance(self.body_size), // free data
                  }
                }
              }
              ReadState::Chunk => {
                if self.buffer.len() > 0 {
                  match chunk::parse(&mut self.buffer)? {
                    chunk::ParseStatus::Chunk(is_last, data) => {
                      if is_last {
                        req.is_last = is_last;
                        self.read_state = ReadState::Request;
                      }

                      match &self.req_func {
                        OnData::Function(f) => {
                          req.data.unsplit(data);
                          let fut = (f)((req, res));
                          self.process_state = ProcessState::Processing(fut.into_future());
                          break;
                        }
                        OnData::Empty => {} // we can skip this data
                      }
                    }
                    chunk::ParseStatus::NotEnoughData => {} // wait for more data
                  };
                }
              }
              ReadState::Request => {
                let mut headers = [httparse::EMPTY_HEADER; 50];
                let mut r = httparse::Request::new(&mut headers);

                // parse available data
                match r.parse(&self.buffer) {
                  Ok(httparse::Status::Partial) => {} // continue reading (not enough data)
                  Ok(httparse::Status::Complete(amt)) => {
                    // we need to reset old body size and headers
                    self.body_size = 0;
                    req.reset_headers(r.headers.len());

                    // always assume that we have data (even if there is no data)
                    self.read_state = ReadState::Body;

                    for header in r.headers.iter() {
                      // make all header's names the same case
                      let header_name = header.name.to_lowercase();

                      if self.read_state != ReadState::Chunk {
                        if header_name == "transfer-encoding" {
                          if &header.value[header.value.len() - 7..header.value.len()] == b"chunked"
                          {
                            self.read_state = ReadState::Chunk;
                          }
                        } else if header_name == "content-length" {
                          //TODO: need to handle errors properly
                          self.body_size = std::str::from_utf8(header.value)
                            .expect("Wrong value in header")
                            .parse::<usize>()
                            .expect("Could not parse usize");
                        }
                      }

                      let mut buf = Vec::with_capacity(header.value.len());
                      unsafe {
                        // we can do unsafe copy here :)
                        buf.bytes_mut()[..header.value.len()].copy_from_slice(header.value)
                      };
                      req.add_header(header_name, buf);
                    }

                    // empty previous function
                    self.req_func = OnData::Empty;

                    let method = r.method.unwrap().to_string();
                    let version = r.version.unwrap();
                    req.init(
                      version,
                      method,
                      r.path.unwrap().parse::<Uri>().unwrap(),
                      self.buffer.split_to(amt),
                    );

                    let fut = unsafe { (*self.router_raw).find((req, res)) };
                    self.process_state = ProcessState::Processing(fut.into_future());
                    break;
                  }
                  Err(_e) => {
                    return Err(std::io::Error::new(
                      std::io::ErrorKind::Other,
                      "Could not parse request",
                    ));
                  }
                }
              }
            }

            if !self.buffer.has_remaining_mut() {
              self.buffer.reserve(1024);
            }

            match self.socket.read_buf(&mut self.buffer)? {
              // 0 socket is closed :)
              Async::Ready(0) => return Ok(Async::Ready((req, res))),
              // We have some data need to check it in next iter
              Async::Ready(_) => {
                self
                  .keep_alive_timer
                  .reset(Instant::now() + Duration::from_secs(10));
              }
              Async::NotReady => {
                // TODO: handle unwrap properly
                match self.keep_alive_timer.poll().unwrap() {
                  Async::Ready(_) => {
                    res.shutdown();
                    return Ok(Async::Ready((req, res)));
                  }
                  Async::NotReady => {
                    // nothing has been read set our state to ready to process new data in next wake up
                    self.process_state = ProcessState::Ready((req, res));
                    return Ok(Async::NotReady);
                  }
                };
              }
            }
          }
        }
      }
    }
  }
}
