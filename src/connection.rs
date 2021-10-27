use std::{
    convert::TryInto,
    io::{BufReader, Read, Write},
    net::TcpStream,
    stream::Stream,
    sync::{Arc, Mutex},
    task::Poll,
    thread,
};

use crate::{
    frame::{Frame, FrameError},
    message::Message,
};

struct TcpWriterHalf(Arc<Mutex<TcpStream>>);

impl std::io::Write for TcpWriterHalf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

struct TcpReaderHalf(Arc<Mutex<TcpStream>>);

impl std::io::Read for TcpReaderHalf {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().read(buf)
    }
}

fn split(s: TcpStream) -> (impl std::io::Read, impl std::io::Write) {
    let arc_s = Arc::new(Mutex::new(s));
    let arc_s_clone = arc_s.clone();
    let writer = TcpWriterHalf(arc_s);
    let reader = TcpReaderHalf(arc_s_clone);
    (reader, writer)
}

pub struct WebSocketConnection {
    stream: TcpStream,
}

impl WebSocketConnection {
    pub fn new(stream: TcpStream) -> Self {
        WebSocketConnection { stream }
    }

    pub fn iter_messages(&mut self) -> MessageIter<impl Read> {
        MessageIter::new(&mut self.stream)
    }

    pub fn on_message<F: Fn(Message) + Send + 'static>(self, f: F) -> Sender<impl Write> {
        let (mut reader, writer) = split(self.stream);

        thread::spawn(move || {
            let iter = MessageIter::new(&mut reader);
            for message in iter.ok() {
                (f)(message);
            }
        });

        Sender { stream: writer }
    }

    pub fn send(&mut self, message: Message) -> Result<(), std::io::Error> {
        let b = Frame::from(message).to_bytes();
        self.stream.write(&b).and(Ok(()))
    }
}

pub struct Sender<W: Write> {
    stream: W,
}

impl<W: Write> Sender<W> {
    pub fn send(&mut self, message: Message) -> Result<(), std::io::Error> {
        let b = Frame::from(message).to_bytes();
        self.stream.write(&b).and(Ok(()))
    }
}

pub struct MessageIter<'a, R: Read> {
    reader: BufReader<&'a mut R>,
    fragmented_seq: Vec<Frame>,
}

impl<'a, R: Read> MessageIter<'a, R> {
    pub fn new(r: &'a mut R) -> Self {
        MessageIter {
            reader: BufReader::new(r),
            fragmented_seq: vec![],
        }
    }

    pub fn ok(self) -> impl Iterator<Item = Message> + 'a {
        self.filter_map(Result::ok)
    }

    fn try_read_one(&mut self) -> Result<Frame, FrameError> {
        Frame::read(&mut self.reader).and_then(|frame| {
            if frame.fin {
                // final message
                if self.fragmented_seq.is_empty() {
                    return Ok(frame);
                }

                self.fragmented_seq.push(frame);

                let big_frame = Frame::from_fragmented(&self.fragmented_seq);

                Ok(big_frame)
            } else {
                self.fragmented_seq.push(frame);
                Err(FrameError::WouldBlock)
            }
        })
    }
}

impl<R: Read> Stream for MessageIter<'_, R> {
    type Item = Message;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.try_read_one() {
            Ok(frame) => Poll::Ready(Some(frame.try_into().unwrap())),
            Err(FrameError::WouldBlock) => Poll::Pending,
            Err(FrameError::Eof) => Poll::Ready(None),
            Err(e) => todo!(),
        }
    }
}

impl<R: Read> Iterator for MessageIter<'_, R> {
    type Item = Result<Message, Box<dyn std::error::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.try_read_one() {
                Ok(frame) => {
                    let message: Message = match frame.try_into() {
                        Ok(message) => message,
                        Err(e) => return Some(Err(e.into())),
                    };
                    return Some(Ok(message));
                }
                Err(FrameError::WouldBlock) => continue, // waiting for more bytes
                Err(FrameError::Eof) => return None,     // nothing to read anymore
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}
