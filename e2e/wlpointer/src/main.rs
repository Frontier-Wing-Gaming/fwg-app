//! One virtual pointer that stays connected for a whole Wayland test session.
//!
//!     wlpointer <output width> <output height>
//!
//! Reads commands from stdin and answers each with "ok": `move X Y`, `click X Y`,
//! `scroll X Y STEPS` (positive = down). Headless sway has no input devices, and tools like
//! wlrctl create a new pointer per command, which the app has not attached to by the time the
//! event arrives; a pointer that lives for the whole session avoids that.

use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_pointer::{Axis, ButtonState};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const BTN_LEFT: u32 = 0x110;

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event,
             _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection,
             _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerManagerV1,
             _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event, _: &(),
             _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwlrVirtualPointerV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerV1,
             _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection,
             _: &QueueHandle<Self>) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<u32> = std::env::args().skip(1).map(|a| a.parse()).collect::<Result<_, _>>()?;
    let [width, height] = args[..] else { return Err("usage: wlpointer <width> <height>".into()) };

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ())?;
    let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ())?;
    let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
    queue.roundtrip(&mut State)?;

    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u32;
    let mut out = std::io::stdout();
    writeln!(out, "ready")?;
    out.flush()?;

    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| parts.get(i).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        let (x, y) = (num(1).clamp(0, width as i32 - 1) as u32, num(2).clamp(0, height as i32 - 1) as u32);
        pointer.motion_absolute(now(), x, y, width, height);
        pointer.frame();
        match parts.first().copied() {
            Some("move") => {}
            Some("click") => {
                queue.roundtrip(&mut State)?;
                std::thread::sleep(Duration::from_millis(50));
                pointer.button(now(), BTN_LEFT, ButtonState::Pressed);
                pointer.frame();
                queue.roundtrip(&mut State)?;
                std::thread::sleep(Duration::from_millis(50));
                pointer.button(now(), BTN_LEFT, ButtonState::Released);
                pointer.frame();
            }
            Some("scroll") => {
                let steps = num(3);
                for _ in 0..steps.unsigned_abs() {
                    pointer.axis_discrete(now(), Axis::VerticalScroll, 15.0 * steps.signum() as f64,
                                          steps.signum());
                    pointer.frame();
                    queue.roundtrip(&mut State)?;
                    std::thread::sleep(Duration::from_millis(30));
                }
            }
            _ => {
                writeln!(out, "error: unknown command {line:?}")?;
                out.flush()?;
                continue;
            }
        }
        queue.roundtrip(&mut State)?;
        writeln!(out, "ok")?;
        out.flush()?;
    }
    Ok(())
}
