use std::{
    convert::TryInto,
    io::{BufReader, Read, Write},
    net::TcpStream,
    stream::Stream,
    task::Poll,
    thread,
};

use crate::{
    frame::{Frame, FrameError},
    message::Message,
    stream_splitter::{split, TcpReaderHalf, TcpWriterHalf},
};

pub struct WebSocketConnection {
    reader: TcpReaderHalf,
    writer: TcpWriterHalf,
}

impl WebSocketConnection {
    pub fn new(stream: TcpStream) -> Self {
        let (reader, writer) = split(stream);
        WebSocketConnection { reader, writer }
    }

    pub fn iter_messages(&mut self) -> MessageIter<impl Read> {
        MessageIter::new(&mut self.reader)
    }

    pub fn on_message(&self, f: impl Fn(Message) + Send + 'static) {
        let mut reader_clone = self.reader.clone();
        thread::spawn(move || {
            let iter = MessageIter::new(&mut reader_clone);
            for message in iter.ok() {
                (f)(message);
            }
        });
    }

    pub fn send(&mut self, message: Message) -> Result<(), std::io::Error> {
        let b = Frame::from(message).to_bytes();
        self.writer.write_all(&b).and(Ok(()))
    }

    pub fn sender(&self) -> Sender<impl Write> {
        Sender {
            writer: self.writer.clone(),
        }
    }
}

pub struct Sender<W: Write> {
    writer: W,
}

impl<W: Write> Sender<W> {
    pub fn send(&mut self, message: Message) -> Result<(), std::io::Error> {
        let fr = Frame::from(message);
        let b = fr.to_bytes();
        self.writer.write_all(&b).and(Ok(()))
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
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.try_read_one() {
            Ok(frame) => Poll::Ready(Some(frame.try_into().unwrap())),
            Err(FrameError::WouldBlock) => Poll::Pending,
            Err(FrameError::Eof) => Poll::Ready(None),
            Err(_e) => todo!(),
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
