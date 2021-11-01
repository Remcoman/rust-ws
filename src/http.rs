use std::{convert::TryFrom, fmt::Display, io::Read, str::from_utf8};

#[cfg(feature = "websocket_key")]
use sha1::Sha1;

#[cfg(feature = "websocket_key")]
static WEBSOCKET_KEY_MAGIC: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

enum State {
    Version,
    Pair,
}

pub struct NameValuePair(Vec<u8>, Vec<u8>);

impl NameValuePair {
    pub fn to_bytes(&self) -> Vec<u8> {
        [self.0.as_slice(), b": ", self.1.as_slice()].concat()
    }

    pub(crate) fn size(&self) -> usize {
        self.0.len() + 2 + self.1.len()
    }
}

impl Display for NameValuePair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.to_bytes();
        write!(f, "{}", from_utf8(&bytes).unwrap())
    }
}

struct Lines<'a> {
    bytes: &'a [u8],
    last_line_index: usize,
}

impl<'a> Lines<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Lines {
            bytes,
            last_line_index: 0,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        for index in self.last_line_index..self.bytes.len() {
            if self.bytes[index] as char == '\r'
                && index + 1 < self.bytes.len()
                && self.bytes[index + 1] as char == '\n'
            {
                let line = &self.bytes[self.last_line_index..index];
                self.last_line_index = index + 2;

                return Some(line);
            }
        }

        None
    }
}

#[derive(Debug)]
pub enum InvalidHTTPHeader {
    MissingTrailingNewLine,
    MissingLeadingLine,
    EOF,
}
impl std::fmt::Display for InvalidHTTPHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingLeadingLine => {
                write!(f, "Missing leading line")
            }
            Self::MissingTrailingNewLine => {
                write!(f, "Missing trailing line")
            }
            Self::EOF => {
                write!(f, "End of file")
            }
        }
    }
}
impl std::error::Error for InvalidHTTPHeader {}

fn trim(x: &[u8]) -> &[u8] {
    let mut s = 0;
    loop {
        if s == (x.len() - 1) || (x[s] as char) != ' ' {
            break;
        }
        s += 1;
    }
    let mut e: usize = x.len() - 1;
    loop {
        if e == 0 || (x[e] as char) != ' ' {
            break;
        }
        e -= 1;
    }
    &x[s..=e]
}

pub struct HTTPHeader {
    leading_line: Vec<u8>,
    pairs: Vec<NameValuePair>,
}

impl HTTPHeader {
    pub fn new() -> Self {
        HTTPHeader {
            leading_line: vec![],
            pairs: vec![],
        }
    }

    pub fn websocket_response() -> Self {
        let mut response = Self::new();
        response.set_leading_line(b"HTTP/1.1 101 Switching Protocols");
        response.add(b"Upgrade", b"websocket");
        response.add(b"Connection", b"Upgrade");
        response
    }

    pub fn websocket_request() -> Self {
        let mut request = Self::new();
        request.set_leading_line(b"GET / HTTP/1.1");
        request.add(b"Connection", b"Upgrade");
        request.add(b"Upgrade", b"websocket");
        request
    }

    pub fn into_websocket_response(&self) -> Self {
        #[allow(unused_mut)]
        let mut response = Self::websocket_response();

        #[cfg(feature = "websocket_key")]
        if let Some(b) = self.get_value(b"Sec-WebSocket-Key") {
            let res = [b, WEBSOCKET_KEY_MAGIC.as_bytes()].concat();
            let mut hasher = Sha1::new();
            hasher.update(&res);
            let hash = base64::encode(hasher.digest().bytes());
            response.add(b"Sec-WebSocket-Accept", &hash);
        }

        response
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let pairs_size = self.pairs.iter().fold(0, |acc, p| acc + p.size());
        let mut lines: Vec<u8> =
            Vec::with_capacity(self.leading_line.len() + 2 + pairs_size + self.pairs.len() * 2 + 2);

        let sep = b"\r\n";

        lines.extend_from_slice(&self.leading_line);
        lines.extend_from_slice(sep);
        for pair in self.pairs.iter() {
            let bytes = pair.to_bytes();
            lines.extend_from_slice(bytes.as_slice());
            lines.extend_from_slice(sep);
        }
        lines.extend_from_slice(sep);

        lines
    }

    pub fn set_leading_line<R: AsRef<[u8]>>(&mut self, value: R) {
        self.leading_line = Vec::from(value.as_ref());
    }

    pub fn get_leading_line(&self) -> &[u8] {
        &self.leading_line
    }

    pub fn get_value<N: AsRef<[u8]>>(&self, name: N) -> Option<&[u8]> {
        let item = self.pairs.iter().find(|pair| pair.0 == name.as_ref());
        item.map(|i| i.1.as_slice())
    }

    pub fn add<N: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, name: N, value: V) {
        self.pairs.push(NameValuePair(
            Vec::from(name.as_ref()),
            Vec::from(value.as_ref()),
        ));
    }

    pub fn is_valid_websocket_response(&self) -> bool {
        let request = self.get_leading_line();
        if request != b"HTTP/1.1 101 Switching Protocols" {
            return false;
        }

        if !matches!(self.get_value(b"Connection"), Some(b"Upgrade")) {
            return false;
        }

        if !matches!(self.get_value(b"Upgrade"), Some(b"websocket")) {
            return false;
        }

        true
    }

    pub fn is_valid_websocket_request(&self) -> bool {
        let request = self.get_leading_line();
        if !request.starts_with(b"GET") {
            return false;
        }

        if !matches!(self.get_value(b"Connection"), Some(b"Upgrade")) {
            return false;
        }

        if !matches!(self.get_value(b"Upgrade"), Some(b"websocket")) {
            return false;
        }

        true
    }

    pub fn read<R: Read>(r: &mut R) -> Result<Self, InvalidHTTPHeader> {
        let mut buf: [u8; 512] = [0; 512];
        let read = r.read(&mut buf).map_err(|_e| InvalidHTTPHeader::EOF)?;

        if read == 0 {
            return Err(InvalidHTTPHeader::EOF);
        }

        Self::from_bytes(&buf)
    }

    fn from_bytes(b: &[u8]) -> Result<Self, InvalidHTTPHeader> {
        let lines = Lines::new(b);

        let mut header = HTTPHeader::new();
        let mut empty_line_found = false;

        let mut s = State::Version;

        for line in lines {
            match s {
                State::Version => {
                    header.set_leading_line(line);

                    s = State::Pair
                }
                State::Pair => {
                    if line.is_empty() {
                        empty_line_found = true;
                        break;
                    }

                    let mut spl = line.split(|c| (*c as char) == ':');
                    let name = trim(spl.next().ok_or(InvalidHTTPHeader::EOF)?);
                    let value = trim(spl.next().ok_or(InvalidHTTPHeader::EOF)?);

                    header.add(name, value);
                }
            }
        }

        if empty_line_found {
            Ok(header)
        } else {
            Err(InvalidHTTPHeader::MissingTrailingNewLine)
        }
    }
}

impl Default for HTTPHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Display for HTTPHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", from_utf8(self.to_bytes().as_slice()).unwrap())
    }
}

impl<'a> IntoIterator for HTTPHeader {
    type Item = NameValuePair;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.pairs.into_iter()
    }
}

impl TryFrom<&[u8]> for HTTPHeader {
    type Error = InvalidHTTPHeader;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        HTTPHeader::from_bytes(value)
    }
}

#[cfg(test)]
mod tests {

    use std::convert::TryFrom;

    use super::HTTPHeader;

    #[test]
    fn can_parse_headers() {
        let s = "GET / HTTP/1.1\r\nHost: 0.0.0.0:3000\r\nConnection: keep-alive\r\nUpgrade-Insecure-Requests: 1\r\n\r\n";

        let header = HTTPHeader::try_from(s.as_bytes()).unwrap();

        assert_eq!(header.get_leading_line(), b"GET / HTTP/1.1");

        assert_eq!(
            header.get_value(b"Connection").unwrap_or(b""),
            b"keep-alive"
        );
        assert_eq!(
            header
                .get_value(b"Upgrade-Insecure-Requests")
                .unwrap_or(b""),
            b"1"
        );
    }

    #[test]
    fn can_create_headers() {
        let mut header = HTTPHeader::new();
        header.set_leading_line(b"HTTP/1.1 101 Switching Protocols");
        header.add(b"Upgrade", b"websocket");
        header.add(b"Connection", b"Upgrade");

        let s = [
            "HTTP/1.1 101 Switching Protocols",
            "Upgrade: websocket",
            "Connection: Upgrade",
            "",
            "",
        ]
        .join("\r\n");

        assert_eq!(header.to_string(), s);
    }
}
