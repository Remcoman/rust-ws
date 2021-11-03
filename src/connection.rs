use std::{
    convert::TryInto,
    io::{BufReader, Read, Write},
    net::TcpStream,
    sync::{
        mpsc::{channel, Sender as ChannelSender},
        Arc, RwLock,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    error::WebSocketError,
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

#[derive(PartialEq, Clone)]
pub enum ConnectionState {
    Open,
    CloseSent,
    Closed,
}

pub struct WebSocketConnection {
    reader: TcpReaderHalf,
    writer: TcpWriterHalf,
    state: Arc<RwLock<ConnectionState>>,
}

impl WebSocketConnection {
    pub fn new(stream: TcpStream) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();

        let (reader, writer) = split(stream);

        WebSocketConnection {
            reader,
            writer,
            state: Arc::new(RwLock::new(ConnectionState::Open)),
        }
    }

    pub fn get_state(&self) -> ConnectionState {
        self.state.read().unwrap().clone()
    }

    pub fn iter_messages(&mut self) -> impl Iterator<Item = Message> + '_ {
        let special_frame_handler = SpecialFrameHandler {
            writer: &mut self.writer,
            state: self.state.clone(),
        };
        FrameIter::new(&mut self.reader, special_frame_handler).messages()
    }

    pub fn on_message(&self, mut f: impl FnMut(Message) + Send + 'static) -> MessageHandler {
        let mut reader_clone = self.reader.clone();
        let mut writer_clone = self.writer.clone();
        let state_clone = self.state.clone();

        let (sender, receiver) = channel();

        let join = thread::spawn(move || {
            // create an iterator which stops when the channel sends a empty tuple
            let stopper =
                std::iter::repeat(()).take_while(|_| !matches!(receiver.try_recv(), Ok(())));

            let special_frame_handler = SpecialFrameHandler {
                writer: &mut writer_clone,
                state: state_clone,
            };

            let iter = FrameIter::new(&mut reader_clone, special_frame_handler);

            for (message, _) in iter.messages().zip(stopper) {
                (f)(message);
            }
        });
        MessageHandler {
            thread: join,
            sender,
        }
    }

    pub fn close(mut self) -> Result<(), WebSocketError> {
        if *self.state.read().unwrap() != ConnectionState::Open {
            return Err(WebSocketError::InvalidConnectionState);
        }

        *self.state.write().unwrap() = ConnectionState::CloseSent;

        let f = Frame::connection_close();

        self.writer
            .write_all(&f.to_bytes())
            .or(Err(WebSocketError::UnknownError))?;

        self.writer.flush().or(Err(WebSocketError::UnknownError))?;

        Ok(())
    }

    pub fn send(&mut self, message: Message) -> Result<(), WebSocketError> {
        if *self.state.read().unwrap() != ConnectionState::Open {
            return Err(WebSocketError::InvalidConnectionState);
        }

        let b = Frame::from(message).to_bytes();
        self.writer
            .write_all(&b)
            .and(Ok(()))
            .or(Err(WebSocketError::UnknownError))
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

pub struct SpecialFrameHandler<'a> {
    writer: &'a mut TcpWriterHalf,
    state: Arc<RwLock<ConnectionState>>,
}

impl<'a> SpecialFrameHandler<'a> {
    fn handle(&mut self, frame: &Frame) -> Result<bool, std::io::Error> {
        match frame.opcode {
            OpCode::ConnectionClose => {
                let state = &*self.state.read().unwrap();

                // confirm received message
                if state == &ConnectionState::Open {
                    self.writer.write_all(&frame.to_bytes())?;
                    self.writer.flush()?;
                }

                // make message final
                if state == &ConnectionState::Open || state == &ConnectionState::CloseSent {
                    self.writer.shutdown()?;
                }

                *self.state.write().unwrap() = ConnectionState::Closed;

                Ok(true)
            }
            OpCode::Ping => {
                let pong = Frame::pong();
                self.writer.write_all(&pong.to_bytes())?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

pub struct FrameIter<'a, R: Read> {
    reader: BufReader<&'a mut R>,
    special_frame_handler: SpecialFrameHandler<'a>,
    fragmented_seq: Vec<Frame>,
}

impl<'a, R: Read> FrameIter<'a, R> {
    pub fn new(r: &'a mut R, special_frame_handler: SpecialFrameHandler<'a>) -> Self {
        FrameIter {
            reader: BufReader::new(r),
            special_frame_handler,
            fragmented_seq: vec![],
        }
    }

    pub fn ok(self) -> impl Iterator<Item = Frame> + 'a {
        self.filter_map(Result::ok)
    }

    pub fn messages(self) -> impl Iterator<Item = Message> + 'a {
        self.ok().filter_map(|f| match f.try_into() {
            Ok(message) => Some(message),
            Err(_e) => None,
        })
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

impl<R: Read> Iterator for FrameIter<'_, R> {
    type Item = Result<Frame, Box<dyn std::error::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.try_read_one() {
                Ok(frame) => match self.special_frame_handler.handle(&frame) {
                    Ok(true) => continue,
                    Ok(false) => return Some(Ok(frame)),
                    Err(e) => return Some(Err(e.into())),
                },
                Err(FrameError::WouldBlock) => continue, // waiting for more bytes
                Err(FrameError::Eof) => return None,     // nothing to read anymore
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}
