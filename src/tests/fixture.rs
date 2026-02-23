use std::os::unix::net::UnixStream;
use std::time::Duration;

use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;

use super::client::Client;
use crate::Monotile;

pub struct Fixture {
    pub event_loop: EventLoop<'static, Monotile>,
    pub mt: Monotile,
    clients: Vec<Client>,
}

impl Fixture {
    pub fn new() -> Self {
        let (event_loop, mut mt) = Monotile::new();

        // headless output
        let output = Output::new(
            "test".into(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "test".into(),
                model: "test".into(),
                serial_number: "0".into(),
            },
        );
        let mode = Mode {
            size: (1000, 800).into(),
            refresh: 60_000,
        };
        output.create_global::<Monotile>(&mt.state.display_handle);
        output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
        output.set_preferred(mode);
        mt.state.add_monitor(output);

        Fixture {
            event_loop,
            mt,
            clients: Vec::new(),
        }
    }

    pub fn add_client(&mut self) -> usize {
        let (server_socket, client_socket) = UnixStream::pair().unwrap();
        self.mt.state.insert_client(server_socket);

        let client = Client::new(client_socket);
        let idx = self.clients.len();
        self.clients.push(client);

        // do initial roundtrip so the client can bind registry globals
        self.roundtrip(idx);
        idx
    }

    pub fn client(&self, idx: usize) -> &Client {
        &self.clients[idx]
    }

    pub fn client_mut(&mut self, idx: usize) -> &mut Client {
        &mut self.clients[idx]
    }

    pub fn roundtrip(&mut self, client_idx: usize) {
        let done = self.clients[client_idx].start_sync();

        for _ in 0..100 {
            self.dispatch();
            self.mt.state.flush_clients();
            self.clients[client_idx].dispatch();

            if done.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
        }
        panic!("roundtrip for client {client_idx} did not complete in 100 iters");
    }

    pub fn dispatch(&mut self) {
        self.event_loop
            .dispatch(Some(Duration::ZERO), &mut self.mt)
            .unwrap();
    }
}
