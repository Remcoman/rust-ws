use std::{
    net::TcpStream,
    sync::{Arc, Mutex},
};

pub struct TcpWriterHalf(Arc<Mutex<TcpStream>>);

impl std::io::Write for TcpWriterHalf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

impl Clone for TcpWriterHalf {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl TcpWriterHalf {
    pub fn shutdown(&self) -> std::io::Result<()> {
        self.0.lock().unwrap().shutdown(std::net::Shutdown::Write)
    }
}

pub struct TcpReaderHalf(Arc<Mutex<TcpStream>>);

impl std::io::Read for TcpReaderHalf {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().read(buf)
    }
}

impl TcpReaderHalf {
    pub fn shutdown(&self) -> std::io::Result<()> {
        self.0.lock().unwrap().shutdown(std::net::Shutdown::Read)
    }
}

impl Clone for TcpReaderHalf {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub fn split(s: TcpStream) -> (TcpReaderHalf, TcpWriterHalf) {
    let arc_s_clone = Arc::new(Mutex::new(s.try_clone().unwrap()));
    let arc_s = Arc::new(Mutex::new(s));
    let writer = TcpWriterHalf(arc_s);
    let reader = TcpReaderHalf(arc_s_clone);
    (reader, writer)
}
