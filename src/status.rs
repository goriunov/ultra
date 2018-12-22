// TODO: Need to add most common statuses
pub enum StatusMessage {
  OK,
  NOT_FOUND,

  // custom status implementation
  Custom(u32, String),
}

impl std::fmt::Display for StatusMessage {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match *self {
      StatusMessage::OK => f.pad("200 OK"),
      StatusMessage::NOT_FOUND => f.pad("404 Not Found"),
      StatusMessage::Custom(c, ref s) => write!(f, "{} {}", c, s),
    }
  }
}