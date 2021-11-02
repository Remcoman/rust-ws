use std::{
    convert::TryInto,
    io::{BufReader, Read, Write},
    net::TcpStream,
    sync::mpsc::{channel, Sender as ChannelSender},
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    frame::{Frame, FrameError, OpCode},
    message::Message,
    stream_splitter::{split, TcpReaderHalf, TcpWriterHalf},
};

pub struct MessageHandler {
    thread: JoinHandle<()>,
    sender: ChannelSender<()>,
}

impl MessageHandler {
    pub fn stop(self) {
        self.sender.send(()).unwrap();
    }

    pub fn join(self) {
        self.thread.join().unwrap()
    }
}

pub struct WebSocketConnection {
    reader: TcpReaderHalf,
    writer: TcpWriterHalf,
}

impl WebSocketConnection {
    pub fn new(stream: TcpStream) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();

        let (reader, writer) = split(stream);

        WebSocketConnection { reader, writer }
    }

    pub fn iter_messages(&mut self) -> MessageIter<impl Read, impl Write> {
        MessageIter::new(&mut self.reader, &mut self.writer)
    }

    pub fn on_message(&self, mut f: impl FnMut(Message) + Send + 'static) -> MessageHandler {
        let mut reader_clone = self.reader.clone();
        let mut writer_clone = self.writer.clone();

        let (sender, receiver) = channel();

        let join = thread::spawn(move || {
            // create an iterator which stops when the channel sends a empty tuple
            let stopper =
                std::iter::repeat(()).take_while(|_| !matches!(receiver.try_recv(), Ok(())));

            let iter = MessageIter::new(&mut reader_clone, &mut writer_clone);

            for (message, _) in iter.ok().zip(stopper) {
                (f)(message);
            }
        });
        MessageHandler {
            thread: join,
            sender,
        }
    }

    pub fn close(self) -> Result<(), std::io::Error> {
        self.reader.shutdown()?;
        self.writer.shutdown()?;
        Ok(())
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

pub struct MessageIter<'a, R: Read, W: Write> {
    reader: BufReader<&'a mut R>,
    writer: &'a mut W,
    fragmented_seq: Vec<Frame>,
}

impl<'a, R: Read, W: Write> MessageIter<'a, R, W> {
    pub fn new(r: &'a mut R, writer: &'a mut W) -> Self {
        MessageIter {
            reader: BufReader::new(r),
            writer,
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

impl<R: Read, W: Write> Iterator for MessageIter<'_, R, W> {
    type Item = Result<Message, Box<dyn std::error::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.try_read_one() {
                Ok(frame) if frame.opcode == OpCode::ConnectionClose => {
                    self.writer.write_all(&frame.to_bytes()).unwrap();
                    self.writer.flush().unwrap();
                    return None;
                }
                Ok(frame) if frame.opcode == OpCode::Ping => {
                    let pong = Frame {
                        opcode: OpCode::Pong,
                        ..frame
                    };
                    self.writer.write_all(&pong.to_bytes()).unwrap();
                    continue;
                }
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
