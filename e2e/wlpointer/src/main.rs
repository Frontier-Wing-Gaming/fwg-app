//! One virtual pointer (and, on KDE, keyboard) that stays connected for a whole Wayland test
//! session.
//!
//!     wlpointer <output width> <output height>
//!
//! Reads commands from stdin and answers each with "ok": `move X Y`, `click X Y`,
//! `scroll X Y STEPS` (positive = down), and on KDE `key CODE 1|0` (an evdev key code, pressed
//! or released). Headless sway has no input devices, and tools like wlrctl create a new pointer
//! per command, which the app has not attached to by the time the event arrives; a pointer that
//! lives for the whole session avoids that.
//!
//! Uses wlroots' virtual pointer (sway) or KWin's fake input (KDE; KWin must run with
//! KWIN_WAYLAND_NO_PERMISSION_CHECKS=1, as it does in e2e/run_linux.sh).

use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_pointer::{Axis, ButtonState};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const BTN_LEFT: u32 = 0x110;

struct State;

macro_rules! ignore_events {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for State {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(),
                     _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(
    wl_seat::WlSeat,
    ZwlrVirtualPointerManagerV1,
    ZwlrVirtualPointerV1,
    OrgKdeKwinFakeInput
);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

enum Device {
    Wlr {
        pointer: ZwlrVirtualPointerV1,
        start: Instant,
        width: u32,
        height: u32,
    },
    Kde(OrgKdeKwinFakeInput),
}

impl Device {
    fn now(start: &Instant) -> u32 {
        start.elapsed().as_millis() as u32
    }

    fn motion(&self, x: u32, y: u32) {
        match self {
            Device::Wlr {
                pointer,
                start,
                width,
                height,
            } => {
                pointer.motion_absolute(Self::now(start), x, y, *width, *height);
                pointer.frame();
            }
            Device::Kde(fake) => fake.pointer_motion_absolute(x as f64, y as f64),
        }
    }

    fn button(&self, pressed: bool) {
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        match self {
            Device::Wlr { pointer, start, .. } => {
                pointer.button(Self::now(start), BTN_LEFT, state);
                pointer.frame();
            }
            Device::Kde(fake) => fake.button(BTN_LEFT, state as u32),
        }
    }

    fn scroll_step(&self, sign: i32) {
        match self {
            Device::Wlr { pointer, start, .. } => {
                pointer.axis_discrete(
                    Self::now(start),
                    Axis::VerticalScroll,
                    15.0 * sign as f64,
                    sign,
                );
                pointer.frame();
            }
            // Fake input has no wheel clicks, only a distance, which apps take as pixels; 45 is
            // about what one click scrolls (the app turns a discrete click into ~42 px).
            Device::Kde(fake) => fake.axis(Axis::VerticalScroll as u32, 45.0 * sign as f64),
        }
    }

    fn key(&self, code: u32, pressed: bool) -> Result<(), String> {
        match self {
            Device::Wlr { .. } => Err("keys are not supported here (use wtype)".into()),
            Device::Kde(fake) => {
                fake.keyboard_key(code, pressed as u32);
                Ok(())
            }
        }
    }
}

fn roundtrip(queue: &mut EventQueue<State>) -> Result<(), Box<dyn std::error::Error>> {
    queue.roundtrip(&mut State)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<u32> = std::env::args()
        .skip(1)
        .map(|a| a.parse())
        .collect::<Result<_, _>>()?;
    let [width, height] = args[..] else {
        return Err("usage: wlpointer <width> <height>".into());
    };

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let device =
        if let Ok(manager) = globals.bind::<ZwlrVirtualPointerManagerV1, _, _>(&qh, 1..=2, ()) {
            let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ())?;
            let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
            Device::Wlr {
                pointer,
                start: Instant::now(),
                width,
                height,
            }
        } else {
            let fake: OrgKdeKwinFakeInput = globals
                .bind(&qh, 4..=5, ())
                .map_err(|e| format!("no virtual pointer or KDE fake input: {e}"))?;
            fake.authenticate("wlpointer".into(), "end-to-end tests".into());
            Device::Kde(fake)
        };
    roundtrip(&mut queue)?;

    let mut out = std::io::stdout();
    writeln!(out, "ready")?;
    out.flush()?;

    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| {
            parts
                .get(i)
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0)
        };
        let (x, y) = (
            num(1).clamp(0, width as i32 - 1) as u32,
            num(2).clamp(0, height as i32 - 1) as u32,
        );
        let result = match parts.first().copied() {
            Some("move") => {
                device.motion(x, y);
                Ok(())
            }
            Some("click") => {
                device.motion(x, y);
                roundtrip(&mut queue)?;
                std::thread::sleep(Duration::from_millis(50));
                device.button(true);
                roundtrip(&mut queue)?;
                std::thread::sleep(Duration::from_millis(50));
                device.button(false);
                Ok(())
            }
            Some("scroll") => {
                device.motion(x, y);
                let steps = num(3);
                for _ in 0..steps.unsigned_abs() {
                    device.scroll_step(steps.signum());
                    roundtrip(&mut queue)?;
                    std::thread::sleep(Duration::from_millis(30));
                }
                Ok(())
            }
            Some("key") => device.key(num(1) as u32, num(2) != 0),
            _ => Err(format!("unknown command {line:?}")),
        };
        roundtrip(&mut queue)?;
        match result {
            Ok(()) => writeln!(out, "ok")?,
            Err(e) => writeln!(out, "error: {e}")?,
        }
        out.flush()?;
    }
    Ok(())
}
