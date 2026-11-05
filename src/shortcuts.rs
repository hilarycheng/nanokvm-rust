//! Window-scoped Wayland shortcut inhibition on eframe's existing connection.
use std::{collections::BTreeMap, io, time::{Duration, Instant}};
use anyhow::{bail, Context, Result};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_client::{backend::{Backend, ObjectId, WaylandError}, Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_callback, wl_registry, wl_seat, wl_surface}};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::{self as manager, ZwpKeyboardShortcutsInhibitManagerV1},
    zwp_keyboard_shortcuts_inhibitor_v1::{self as inhibitor, ZwpKeyboardShortcutsInhibitorV1},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status { Discovering, Ready, Pending, Active, Unavailable(String) }

struct Seat {
    proxy: wl_seat::WlSeat,
    inhibitor: Option<ZwpKeyboardShortcutsInhibitorV1>,
    active: bool,
}

#[derive(Default)]
struct State {
    manager: Option<(u32, ZwpKeyboardShortcutsInhibitManagerV1)>,
    seats: BTreeMap<u32, Seat>,
    discovered: bool,
    // shortcut: failures require restarting Focus Mode's owner; add explicit retry if recovery becomes necessary.
    failure: Option<String>,
}

pub struct Shortcuts {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    surface: wl_surface::WlSurface,
    requested: Option<Instant>,
}

impl Shortcuts {
    /// The display and surface must outlive this controller, including its drop.
    pub unsafe fn new(display: RawDisplayHandle, window: RawWindowHandle) -> Result<Self> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) = (display, window) else {
            bail!("Focus Mode requires a native Wayland window");
        };
        // eframe owns these handles; we never destroy its display or surface.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let id = unsafe { ObjectId::from_ptr(wl_surface::WlSurface::interface(), window.surface.as_ptr().cast()) }?;
        let surface = wl_surface::WlSurface::from_id(&connection, id)?;
        let queue = connection.new_event_queue();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());
        connection.display().sync(&qh, ());
        let controller = Self { connection, queue, state: State::default(), surface, requested: None };
        controller.flush()?;
        Ok(controller)
    }

    fn flush(&self) -> Result<()> {
        match self.connection.flush() {
            Ok(()) => Ok(()),
            Err(WaylandError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(error) => Err(error).context("Wayland shortcut connection failed"),
        }
    }

    pub fn status(&self) -> Status {
        if let Some(error) = &self.state.failure { return Status::Unavailable(error.clone()); }
        if !self.state.discovered { return Status::Discovering; }
        if self.state.manager.is_none() { return Status::Unavailable("Compositor does not support shortcut inhibition".into()); }
        if self.state.seats.is_empty() { return Status::Unavailable("No Wayland input seat available".into()); }
        if self.requested.is_some() {
            if self.state.seats.values().any(|seat| seat.active) { Status::Active } else { Status::Pending }
        } else { Status::Ready }
    }

    pub fn poll(&mut self) -> Status {
        // Winit reads the shared socket; dispatch only our queue without blocking or round trips.
        if let Err(error) = self.queue.dispatch_pending(&mut self.state).map(|_| ()).map_err(anyhow::Error::from)
            .and_then(|_| self.flush()) {
            self.state.failure = Some(error.to_string());
        }
        if self.status() == Status::Pending && self.requested.is_some_and(|start| start.elapsed() >= Duration::from_secs(2)) {
            self.state.failure = Some("Compositor did not activate Focus Mode; use Non-Focus Mode".into());
        }
        self.status()
    }

    pub fn request(&mut self) -> Result<()> {
        if self.requested.is_some() { return Ok(()); }
        if self.status() != Status::Ready { bail!("Shortcut inhibition is not ready"); }
        let manager = &self.state.manager.as_ref().unwrap().1;
        let qh = self.queue.handle();
        for seat in self.state.seats.values_mut() {
            seat.inhibitor = Some(manager.inhibit_shortcuts(&self.surface, &seat.proxy, &qh, ()));
        }
        self.requested = Some(Instant::now());
        self.flush()
    }

    pub fn release(&mut self) {
        self.requested = None;
        for seat in self.state.seats.values_mut() {
            if let Some(inhibitor) = seat.inhibitor.take() { inhibitor.destroy(); }
            seat.active = false;
        }
        if let Err(error) = self.flush() { self.state.failure = Some(error.to_string()); }
    }
}

impl Drop for Shortcuts {
    fn drop(&mut self) {
        self.release();
        for seat in self.state.seats.values() {
            if seat.proxy.version() >= 5 { seat.proxy.release(); }
        }
        if let Some((_, manager)) = self.state.manager.take() { manager.destroy(); }
        let _ = self.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event,
        _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        match event {
            wl_registry::Event::Global { name, interface, .. } if interface == ZwpKeyboardShortcutsInhibitManagerV1::interface().name => {
                if state.manager.is_none() { state.manager = Some((name, registry.bind(name, 1, qh, ()))); }
            }
            wl_registry::Event::Global { name, interface, version } if interface == wl_seat::WlSeat::interface().name => {
                if state.seats.values().any(|seat| seat.inhibitor.is_some()) {
                    state.failure = Some("Wayland input seats changed; Focus Mode released".into());
                }
                state.seats.insert(name, Seat { proxy: registry.bind(name, version.min(5), qh, ()), inhibitor: None, active: false });
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(mut seat) = state.seats.remove(&name) {
                    if let Some(inhibitor) = seat.inhibitor.take() {
                        inhibitor.destroy();
                        state.failure = Some("Wayland input seat removed; Focus Mode released".into());
                    }
                    if seat.proxy.version() >= 5 { seat.proxy.release(); }
                }
                if state.manager.as_ref().is_some_and(|(id, _)| *id == name) {
                    state.manager.take().unwrap().1.destroy();
                    state.failure = Some("Shortcut inhibition is no longer available".into());
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>) { state.discovered = true; }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(state: &mut Self, proxy: &ZwpKeyboardShortcutsInhibitorV1, event: inhibitor::Event,
        _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let Some(seat) = state.seats.values_mut().find(|seat| seat.inhibitor.as_ref() == Some(proxy)) {
            match event {
                inhibitor::Event::Active => seat.active = true,
                inhibitor::Event::Inactive => {
                    seat.active = false;
                    state.failure = Some("Compositor restored local shortcuts; Focus Mode released".into());
                }
                _ => {}
            }
        }
    }
}

wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State: ignore manager::ZwpKeyboardShortcutsInhibitManagerV1);

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{io::{Read, Write}, os::unix::net::UnixStream, ptr::NonNull};
    use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
    use wayland_client::protocol::wl_compositor;

    struct Host;
    wayland_client::delegate_noop!(Host: ignore wl_registry::WlRegistry);
    wayland_client::delegate_noop!(Host: ignore wl_compositor::WlCompositor);
    wayland_client::delegate_noop!(Host: ignore wl_surface::WlSurface);

    // Minimal wire peer exercises the production guest backend without a desktop compositor.
    pub(crate) struct Peer {
        socket: UnixStream,
        host: Connection,
        _queue: EventQueue<Host>,
        surface: wl_surface::WlSurface,
        registry: u32,
    }

    impl Peer {
        pub(crate) fn new() -> Self {
            let (client, socket) = UnixStream::pair().unwrap();
            socket.set_nonblocking(true).unwrap();
            let host = Connection::from_socket(client).unwrap();
            let queue = host.new_event_queue();
            let registry = host.display().get_registry(&queue.handle(), ());
            let compositor: wl_compositor::WlCompositor = registry.bind(3, 1, &queue.handle(), ());
            let surface = compositor.create_surface(&queue.handle(), ());
            host.flush().unwrap();
            let mut peer = Self { socket, host, _queue: queue, surface, registry: 0 };
            peer.requests();
            peer
        }

        pub(crate) fn controller(&mut self, supported: bool) -> Shortcuts {
            // Peer owns the display/surface and outlives the returned controller in each test.
            let mut controller = unsafe { Shortcuts::new(
                RawDisplayHandle::Wayland(WaylandDisplayHandle::new(NonNull::new(self.host.backend().display_ptr().cast()).unwrap())),
                RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::new(self.surface.id().as_ptr().cast()).unwrap())),
            ) }.unwrap();
            assert_eq!(controller.status(), Status::Discovering);
            let requests = self.requests();
            let registry = requests.iter().find(|(id, op, _)| *id == 1 && *op == 1).unwrap().2[0];
            self.registry = registry;
            let callback = requests.iter().find(|(id, op, _)| *id == 1 && *op == 0).unwrap().2[0];
            self.global(registry, 1, "wl_seat", 5);
            if supported { self.global(registry, 2, ZwpKeyboardShortcutsInhibitManagerV1::interface().name, 1); }
            self.event(callback, 0, &[0]);
            self.read();
            controller.poll();
            self.requests();
            controller
        }

        pub(crate) fn requests(&mut self) -> Vec<(u32, u16, Vec<u32>)> {
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                match self.socket.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("{error}"),
                }
            }
            let mut requests = Vec::new();
            while !bytes.is_empty() {
                let id = u32::from_ne_bytes(bytes[..4].try_into().unwrap());
                let header = u32::from_ne_bytes(bytes[4..8].try_into().unwrap());
                let length = (header >> 16) as usize;
                assert!(length >= 8 && length <= bytes.len());
                let body = bytes[8..length].chunks_exact(4).map(|w| u32::from_ne_bytes(w.try_into().unwrap())).collect();
                requests.push((id, header as u16, body));
                bytes.drain(..length);
            }
            requests
        }

        fn global(&mut self, registry: u32, name: u32, interface: &str, version: u32) {
            let mut body = vec![name, interface.len() as u32 + 1];
            let mut string = interface.as_bytes().to_vec();
            string.push(0);
            while string.len() % 4 != 0 { string.push(0); }
            body.extend(string.chunks_exact(4).map(|w| u32::from_ne_bytes(w.try_into().unwrap())));
            body.push(version);
            self.event(registry, 0, &body);
        }

        fn event(&mut self, id: u32, opcode: u16, body: &[u32]) {
            let mut message = vec![id, (((body.len() + 2) * 4) as u32) << 16 | opcode as u32];
            message.extend_from_slice(body);
            let bytes: Vec<_> = message.into_iter().flat_map(u32::to_ne_bytes).collect();
            self.socket.write_all(&bytes).unwrap();
        }

        fn read(&self) { self.host.prepare_read().unwrap().read().unwrap(); }

        pub(crate) fn activate(&mut self, controller: &mut Shortcuts) -> u32 {
            let requests = self.requests();
            let (_, _, body) = requests.iter().find(|(_, op, body)| *op == 1 && body.len() == 3).expect("inhibit request");
            assert_eq!(body[1], self.surface.id().protocol_id());
            let id = body[0];
            self.event(id, 0, &[]);
            self.read();
            assert_eq!(controller.poll(), Status::Active);
            id
        }

        pub(crate) fn revoke(&mut self, id: u32, controller: &mut Shortcuts) {
            self.event(id, 1, &[]);
            self.read();
            assert!(matches!(controller.poll(), Status::Unavailable(_)));
        }
    }

    #[test]
    fn guest_connection_inhibits_exact_surface_and_releases_without_duplicates() {
        let mut peer = Peer::new();
        let mut controller = peer.controller(true);
        assert_eq!(controller.status(), Status::Ready);
        controller.request().unwrap();
        assert_eq!(controller.status(), Status::Pending);
        let id = peer.activate(&mut controller);
        controller.request().unwrap();
        assert!(peer.requests().is_empty());
        controller.release();
        assert_eq!(controller.status(), Status::Ready);
        assert_eq!(peer.requests(), vec![(id, 0, vec![])]);
        controller.release();
        assert!(peer.requests().is_empty());
        controller.request().unwrap();
        let id = peer.activate(&mut controller);
        drop(controller);
        assert!(peer.requests().contains(&(id, 0, vec![])));
        assert!(peer.host.flush().is_ok());
    }

    #[test]
    fn seat_or_manager_changes_fail_closed_and_destroy_existing_inhibitors() {
        for changed in [1, 2, 4] {
            let mut peer = Peer::new();
            let mut controller = peer.controller(true);
            controller.request().unwrap();
            let id = peer.activate(&mut controller);
            if changed == 4 { peer.global(peer.registry, 4, "wl_seat", 5); }
            else { peer.event(peer.registry, 1, &[changed]); }
            peer.read();
            assert!(matches!(controller.poll(), Status::Unavailable(_)));
            controller.release();
            assert!(peer.requests().contains(&(id, 0, vec![])));
        }
    }

    #[test]
    fn unsupported_revoked_and_unacknowledged_requests_do_not_claim_focus() {
        let mut peer = Peer::new();
        let mut controller = peer.controller(false);
        assert!(matches!(controller.status(), Status::Unavailable(_)));
        assert!(controller.request().is_err());
        assert!(peer.requests().is_empty());
        drop(controller);
        let mut controller = peer.controller(true);
        controller.request().unwrap();
        let id = peer.activate(&mut controller);
        peer.revoke(id, &mut controller);
        controller.release();
        assert!(peer.requests().contains(&(id, 0, vec![])));
        drop(controller);
        let mut controller = peer.controller(true);
        controller.request().unwrap();
        let requests = peer.requests();
        let id = requests.iter().find(|(_, op, body)| *op == 1 && body.len() == 3).unwrap().2[0];
        controller.requested = Some(Instant::now() - Duration::from_secs(3));
        assert!(matches!(controller.poll(), Status::Unavailable(_)));
        controller.release();
        assert!(peer.requests().contains(&(id, 0, vec![])));
    }
}
