// implement router logic
use crate::method;
use crate::request;
use crate::response;

use hashbrown;
use tokio::prelude::*;

use std::sync::Arc;

pub type ReturnFuture = Box<dyn Future<Item = response::Response, Error = ()> + Send + Sync>;

type StoreFunc = Box<
  dyn Fn(request::Request) -> Box<dyn Future<Item = response::Response, Error = ()> + Send + Sync>
    + Send
    + Sync,
>;

pub struct Node {
  param: Option<&'static str>,
  method: Option<StoreFunc>,
  children: Option<hashbrown::HashMap<&'static str, Node>>,
}

impl Node {
  pub fn default() -> Node {
    Node {
      param: None,
      method: None,
      children: None,
    }
  }

  pub fn set_func(&mut self, func: StoreFunc) {
    self.method = Some(func);
  }

  pub fn add_child(&mut self, seg: &'static str, param: Option<&'static str>) -> &mut Node {
    if self.children.is_none() {
      self.children = Some(hashbrown::HashMap::new())
    }

    let node_map = self.children.as_mut().unwrap();

    // if key exist then return existing node ref
    if node_map.contains_key(seg) {
      return node_map.get_mut(seg).unwrap();
    }

    // create new if node
    node_map.insert(
      seg,
      Node {
        param,
        method: None,
        children: None,
      },
    );
    // this item is just added
    node_map.get_mut(seg).unwrap()
  }
}

pub struct Router {
  get: Node,
  put: Node,
  post: Node,
  head: Node,
  patch: Node,
  delete: Node,
  options: Node,
  routes: Node,
  default: Option<StoreFunc>,
}

impl Router {
  pub fn new() -> Router {
    Router {
      default: None,
      // for now leave this one in
      routes: Node::default(),
      // create separate bucket for each method (not fun) :(
      get: Node::default(),
      put: Node::default(),
      post: Node::default(),
      head: Node::default(),
      patch: Node::default(),
      delete: Node::default(),
      options: Node::default(),
    }
  }

  pub fn build(self) -> Arc<Self> {
    Arc::new(self)
  }

  // rewrite and optimize find algorithm
  // need to re implement find method
  pub fn find(&self, mut req: request::Request) -> ReturnFuture {
    // !! we need to do a lot of optimization for search
    // and add additional router parsing things
    let mut node = &self.routes;
    let mut not_found: bool = false;

    // this thing does not work properly
    if req.uri().path() == "/" {
      return (node.method.as_ref().unwrap())(req);
    }

    // need to add capacity to do not relocate
    // how do we return
    let mut params: Vec<(&'static str, String)> = Vec::new();

    for seg in req.uri().path().split('/') {
      if seg.len() > 0 {
        if node.children.is_none() {
          not_found = true;
          break;
        }

        let children = node.children.as_ref().unwrap();

        let mut found_node = children.get(seg);

        if found_node.is_none() {
          // search for param first
          found_node = children.get(":");

          if found_node.is_none() {
            // if we found at least star then load star route
            found_node = children.get("*");

            if found_node.is_some() {
              node = found_node.unwrap();
              break;
            }

            not_found = true;
            break;
          }

          params.push((found_node.unwrap().param.unwrap(), seg.to_string()));
        }

        node = found_node.unwrap();
      }
    }

    req.set_params(Some(params));

    // if route was not found then return
    if not_found {
      return (self.default.as_ref().unwrap())(req);
    }

    match node.method.as_ref() {
      Some(func) => (func)(req),
      None => {
        // if none then load 404 route
        (self.default.as_ref().unwrap())(req)
      }
    }
  }

  pub fn add<F>(&mut self, method: &str, path: &'static str, func: F)
  where
    F: Fn(request::Request) -> Box<Future<Item = response::Response, Error = ()> + Send + Sync>
      + Send
      + Sync
      + 'static,
  {
    // use proper enum
    let mut node = match method {
      method::GET => &mut self.get,
      method::PUT => &mut self.put,
      method::POST => &mut self.post,
      method::HEAD => &mut self.head,
      method::PATCH => &mut self.patch,
      method::DELETE => &mut self.delete,
      method::OPTIONS => &mut self.options,
      _ => &mut self.routes,
    };

    match path {
      "*" => {
        // default or 404 case we must have one for now :(
        self.default = Some(Box::new(func));
      }
      "/" => {
        // handle / case
        node.set_func(Box::new(func));
      }
      _ => {
        // handle rest of the cases
        for seg in path.split('/') {
          if !seg.is_empty() {
            let mut seg_arr = seg.chars();
            // check if path is param
            if seg_arr.next() == Some(':') {
              node = node.add_child(":", Some(seg_arr.as_str()));
              continue;
            }
            node = node.add_child(seg, None);
          }
        }

        node.set_func(Box::new(func));
      }
    }
  }
}

///
///
/// temp fmt for above struct(s)
///
///
impl std::fmt::Debug for Router {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(
      f,
      "Router {{ \n   get: {:#?}, \npost: {:#?} \ndelete:{:#?}\nput:{:#?} \nroutes:{:#?} \n}}",
      self.get, self.post, self.delete, self.put, self.routes
    )
  }
}

impl std::fmt::Debug for Node {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(
      f,
      "Node {{ \n\tchildren: {:#?}, \n\tmethod: {:#?} \n\tparam:{:#?}\n}}",
      self.children,
      self.method.is_some(),
      self.param
    )
  }
}
